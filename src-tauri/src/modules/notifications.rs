//! Other apps' toast notifications, shown in the island for a few seconds each.
//!
//! A poller reads the Windows user notification listener every `POLL_INTERVAL`, and only while the
//! feature is on. The change event fails for unpackaged apps, so there is nothing to subscribe to.
//! A `Tracker` decides which toasts are new, and a `Queue` shows them one at a time, each for
//! `SHOW_FOR`. Both are pure, so the rules are tested without COM or a clock.
//!
//! Notification text lives in memory only. The one thing kept on disk is the list of app ids and
//! names for the settings window, in `notification-apps.json` next to `config.json`.

mod listener;

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use super::{Module, ModuleContext};
use crate::config::{self, ConfigFile};
use crate::modules::controls::ComGuard;
use crate::modules::media::art_data_uri;
use crate::state::{self, SharedIsland};
use listener::{Listener, Toast};

/// One notification on screen. Mirrors Notice in src/lib/notifications.ts.
const SHOW_EVENT: &str = "notifications://show";
/// The settings snapshot: access and the apps seen so far. Mirrors NotificationsSnapshot.
const UPDATE_EVENT: &str = "notifications://update";
const APPS_FILE: &str = "notification-apps.json";
/// How often the listener is read while the feature is on. A toast shows up this late at most.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
/// How long each notification stays up before the next one, or before the island gives way.
pub const SHOW_FOR: Duration = Duration::from_secs(5);
/// Notifications waiting behind the one on screen. The oldest waiting one is dropped beyond this.
pub const MAX_WAITING: usize = 5;
/// Toast ids already seen. Windows numbers toasts upwards within a session, so the newest are kept.
const SEEN_CAP: usize = 256;
/// Apps remembered for the settings list. The oldest is forgotten beyond this.
const MAX_APPS: usize = 50;
/// Text limits, in characters. The island also cuts to one line, these keep the messages small.
pub const TITLE_CHARS: usize = 80;
pub const BODY_CHARS: usize = 160;

/// Whether Windows lets Notchy read notifications. `Unknown` until the listener has been read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Unknown,
    Allowed,
    Denied,
    Unspecified,
}

/// An app that has sent a notification. The id is its AppUserModelId, the key `deniedApps` uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeenApp {
    pub id: String,
    pub name: String,
    /// The app's logo as a data URI, once it has been read. Older files have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The settings view. Mirrors NotificationsSnapshot in src/lib/notifications.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub access: Access,
    pub apps: Vec<SeenApp>,
}

/// A notification as the island shows it. Mirrors Notice in src/lib/notifications.ts. `seq` counts
/// up from one per notification, so the frontend can tell a new one from the one before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub seq: u64,
    pub app: String,
    pub title: String,
    pub body: String,
    /// The app's logo as a data URI, when it is known. The island shows its initial otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// A toast that passed the filters and waits for the queue. The queue numbers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub app: String,
    pub title: String,
    pub body: String,
    pub icon: Option<String>,
}

/// Decides which toasts are new. The first poll after a reset records what is already there and
/// reports nothing, so turning the feature on never floods the island with old messages.
#[derive(Debug, Default)]
pub struct Tracker {
    seen: BTreeSet<u32>,
    primed: bool,
}

impl Tracker {
    /// Forgets the baseline. The next poll records the toasts present and shows none of them.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The indexes into `ids` that are new, in the order given. Every id is recorded as seen.
    pub fn fresh(&mut self, ids: &[u32]) -> Vec<usize> {
        let fresh = if self.primed {
            ids.iter()
                .enumerate()
                .filter(|(_, id)| !self.seen.contains(id))
                .map(|(index, _)| index)
                .collect()
        } else {
            Vec::new()
        };
        self.seen.extend(ids.iter().copied());
        while self.seen.len() > SEEN_CAP {
            self.seen.pop_first();
        }
        self.primed = true;
        fresh
    }
}

/// What the island should do after a queue step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Show this notification now. Its payload goes out before the island state changes.
    Show(Notice),
    /// Nothing is left to show. The island gives the notification state up.
    Hide,
}

/// The notifications on screen and waiting. One shows at a time, each for `SHOW_FOR`. A newer one
/// waits its turn rather than cutting the current one short. Every method takes the instant to act
/// at, so the tests never read the clock.
#[derive(Debug, Default)]
pub struct Queue {
    next_seq: u64,
    waiting: VecDeque<Notice>,
    /// The notification on screen, and when it gives way.
    showing: Option<(Notice, Instant)>,
}

impl Queue {
    /// Adds a notification. It shows at once when nothing is up. Otherwise it waits, and beyond
    /// `MAX_WAITING` the oldest waiting one is dropped.
    pub fn push(&mut self, pending: Pending, now: Instant) -> Vec<Step> {
        self.next_seq += 1;
        let notice = Notice {
            seq: self.next_seq,
            app: pending.app,
            icon: pending.icon,
            title: pending.title,
            body: pending.body,
        };
        if self.showing.is_some() {
            self.waiting.push_back(notice);
            while self.waiting.len() > MAX_WAITING {
                self.waiting.pop_front();
            }
            Vec::new()
        } else {
            self.show(notice, now)
        }
    }

    /// Gives way once the one on screen has had its time. The next waiting one shows, or the
    /// island hides when none is left.
    pub fn advance(&mut self, now: Instant) -> Vec<Step> {
        match &self.showing {
            Some((_, until)) if now >= *until => {}
            _ => return Vec::new(),
        }
        self.showing = None;
        match self.waiting.pop_front() {
            Some(next) => self.show(next, now),
            None => vec![Step::Hide],
        }
    }

    /// Drops everything, for when the feature is turned off. Hides the one on screen.
    pub fn clear(&mut self) -> Vec<Step> {
        self.waiting.clear();
        match self.showing.take() {
            Some(_) => vec![Step::Hide],
            None => Vec::new(),
        }
    }

    /// When the notification on screen gives way, or None when nothing is on screen.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.showing.as_ref().map(|(_, until)| *until)
    }

    fn show(&mut self, notice: Notice, now: Instant) -> Vec<Step> {
        self.showing = Some((notice.clone(), now + SHOW_FOR));
        vec![Step::Show(notice)]
    }
}

/// Collapses runs of whitespace to single spaces, and trims both ends.
pub fn tidy(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Cuts `text` to at most `max` characters. A cut ends with an ellipsis. Counts characters, not
/// bytes, so a cut never splits a letter.
pub fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The title and body of a toast, from its text lines. The first non-empty line is the title and
/// the rest, joined with spaces, are the body. Repeated lines are dropped, since some toasts repeat
/// their text. None when the toast has no text at all.
pub fn compose(lines: &[String]) -> Option<(String, String)> {
    let mut parts: Vec<String> = Vec::new();
    for line in lines {
        let line = tidy(line);
        if !line.is_empty() && !parts.contains(&line) {
            parts.push(line);
        }
    }
    let mut parts = parts.into_iter();
    let title = parts.next()?;
    let body = parts.collect::<Vec<_>>().join(" ");
    Some((truncate(&title, TITLE_CHARS), truncate(&body, BODY_CHARS)))
}

/// Whether an app's notifications are shown. Denied ids come from the settings. AppUserModelIds
/// are compared without case, as Windows compares them.
pub fn allowed(app_id: &str, denied: &[String]) -> bool {
    !denied.iter().any(|id| id.eq_ignore_ascii_case(app_id))
}

/// Records an app for the settings list. Returns true when the list changed. The first name seen
/// for an id is kept, and the oldest app is forgotten beyond `MAX_APPS`.
pub fn remember(apps: &mut Vec<SeenApp>, id: &str, name: &str) -> bool {
    if id.is_empty() || apps.iter().any(|app| app.id.eq_ignore_ascii_case(id)) {
        return false;
    }
    apps.push(SeenApp {
        id: id.to_string(),
        name: name.to_string(),
        icon: None,
    });
    if apps.len() > MAX_APPS {
        apps.remove(0);
    }
    true
}

/// Whether the app with `id` has been seen and still has no logo.
pub fn needs_icon(apps: &[SeenApp], id: &str) -> bool {
    apps.iter()
        .any(|app| app.id.eq_ignore_ascii_case(id) && app.icon.is_none())
}

/// Stores a logo for the app with `id`. Reports whether anything changed.
pub fn set_icon(apps: &mut [SeenApp], id: &str, icon: String) -> bool {
    match apps.iter_mut().find(|app| app.id.eq_ignore_ascii_case(id)) {
        Some(app) if app.icon.as_deref() != Some(icon.as_str()) => {
            app.icon = Some(icon);
            true
        }
        _ => false,
    }
}

/// The logo of the app with `id`, if one has been read.
pub fn icon_of(apps: &[SeenApp], id: &str) -> Option<String> {
    apps.iter()
        .find(|app| app.id.eq_ignore_ascii_case(id))
        .and_then(|app| app.icon.clone())
}

/// The messages from the poller and the settings window, read by the scheduler.
enum Message {
    Incoming(Pending),
    /// The feature was turned off. Drop whatever is queued.
    Clear,
}

/// Shared state. The poller writes it, and the settings command reads it.
pub struct Hub {
    snapshot: Mutex<Snapshot>,
    apps_file: PathBuf,
    /// Wakes the poller when the notification settings change.
    wake: Sender<()>,
}

impl Hub {
    fn lock(&self) -> MutexGuard<'_, Snapshot> {
        self.snapshot.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct NotificationsModule;

impl Module for NotificationsModule {
    fn name(&self) -> &'static str {
        "notifications"
    }

    fn start(&self, ctx: ModuleContext) {
        let apps_file = ctx.app.state::<ConfigFile>().0.with_file_name(APPS_FILE);
        let (wake, wakes) = mpsc::channel();
        let hub = Arc::new(Hub {
            snapshot: Mutex::new(Snapshot {
                access: Access::Unknown,
                apps: load_apps(&apps_file),
            }),
            apps_file,
            wake,
        });
        ctx.app.manage(hub.clone());

        let (shows, pending) = mpsc::channel();
        let scheduler_app = ctx.app.clone();
        let island = ctx.island.clone();
        thread::spawn(move || run_scheduler(scheduler_app, island, pending));

        let poller_app = ctx.app.clone();
        thread::spawn(move || run_poller(poller_app, hub, wakes, shows));
    }
}

/// Called when the notification settings change. Wakes the poller, so it starts, stops or re-reads
/// the denied list at once.
pub fn reapply(app: &AppHandle) {
    if let Some(hub) = app.try_state::<Arc<Hub>>() {
        let _ = hub.wake.send(());
    }
}

/// The settings view: whether access is allowed, and the apps seen so far.
#[tauri::command]
pub fn notifications_snapshot(hub: State<'_, Arc<Hub>>) -> Snapshot {
    hub.lock().clone()
}

/// Owns the poller's state: the baseline, and whether access was asked for. Runs for the life of
/// the app. Blocks while the feature is off, so it reads nothing while off.
fn run_poller(app: AppHandle, hub: Arc<Hub>, wakes: Receiver<()>, shows: Sender<Message>) {
    let Some(_com) = ComGuard::init() else {
        return;
    };
    let Some(listener) = Listener::open() else {
        return;
    };

    let mut tracker = Tracker::default();
    let mut on = false;
    let mut asked = false;
    // Apps whose logo was already asked for in this run, found or not, so a failure is not retried
    // on every poll.
    let mut logos_tried = HashSet::new();
    loop {
        if !config::notifications_enabled(&app) {
            if on {
                on = false;
                let _ = shows.send(Message::Clear);
            }
            if wakes.recv().is_err() {
                return;
            }
            continue;
        }
        if !on {
            on = true;
            tracker.reset();
            asked = false;
        }

        poll(&app, &hub, &listener, &mut tracker, &mut asked, &mut logos_tried, &shows);

        match wakes.recv_timeout(POLL_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// One read of the listener. Asks for access once per time the feature is turned on, when Windows
/// has not decided yet. Without access, nothing is read.
fn poll(
    app: &AppHandle,
    hub: &Hub,
    listener: &Listener,
    tracker: &mut Tracker,
    asked: &mut bool,
    logos_tried: &mut HashSet<String>,
    shows: &Sender<Message>,
) {
    let mut access = listener.access();
    if access == Access::Unspecified && !*asked {
        *asked = true;
        access = listener.request_access();
    }
    update(app, hub, |snapshot| {
        let changed = snapshot.access != access;
        snapshot.access = access;
        changed
    });
    if access != Access::Allowed {
        return;
    }

    let Ok(toasts) = listener.toasts() else {
        return;
    };
    update(app, hub, |snapshot| {
        let mut changed = false;
        for toast in &toasts {
            changed |= remember(&mut snapshot.apps, &toast.app_id, &toast.app_name);
        }
        changed
    });

    // Each app's logo is read once, the first time it is seen without one.
    for toast in &toasts {
        let key = toast.app_id.to_lowercase();
        if logos_tried.contains(&key) || !needs_icon(&hub.lock().apps, &toast.app_id) {
            continue;
        }
        logos_tried.insert(key);
        if let Some(icon) = listener::logo(toast).and_then(|bytes| art_data_uri(&bytes)) {
            update(app, hub, |snapshot| set_icon(&mut snapshot.apps, &toast.app_id, icon));
        }
    }

    let ids: Vec<u32> = toasts.iter().map(|toast| toast.id).collect();
    let denied = config::denied_apps(app);
    let mut fresh: Vec<&Toast> = tracker
        .fresh(&ids)
        .into_iter()
        .map(|index| &toasts[index])
        .collect();
    fresh.sort_by_key(|toast| toast.id);
    for toast in fresh {
        if !allowed(&toast.app_id, &denied) {
            continue;
        }
        if let Some((title, body)) = compose(&toast.lines) {
            let pending = Pending {
                app: toast.app_name.clone(),
                title,
                body,
                icon: icon_of(&hub.lock().apps, &toast.app_id),
            };
            let _ = shows.send(Message::Incoming(pending));
        }
    }
}

/// Applies `change` to the snapshot. When it reports a change, the apps are saved and the settings
/// window is told. The lock is not held while either happens.
fn update(app: &AppHandle, hub: &Hub, change: impl FnOnce(&mut Snapshot) -> bool) {
    let next = {
        let mut snapshot = hub.lock();
        if !change(&mut snapshot) {
            return;
        }
        snapshot.clone()
    };
    save_apps(&hub.apps_file, &next.apps);
    let _ = app.emit(UPDATE_EVENT, &next);
}

/// Owns the queue and the island's notification state. Waits for the next message or the next
/// give-way, whichever comes first.
fn run_scheduler(app: AppHandle, island: SharedIsland, inbox: Receiver<Message>) {
    let mut queue = Queue::default();
    loop {
        let received = match queue.next_deadline() {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let now = Instant::now();
        let mut steps = match received {
            Ok(Message::Incoming(pending)) => queue.push(pending, now),
            Ok(Message::Clear) => queue.clear(),
            Err(RecvTimeoutError::Timeout) => Vec::new(),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        steps.extend(queue.advance(now));

        for step in steps {
            match step {
                Step::Show(notice) => {
                    let _ = app.emit(SHOW_EVENT, &notice);
                    state::commit(&app, &island, |machine| machine.set_notification(true));
                }
                Step::Hide => {
                    state::commit(&app, &island, |machine| machine.set_notification(false));
                }
            }
        }
    }
}

/// The apps saved last time. A missing or unreadable file gives none.
fn load_apps(path: &Path) -> Vec<SeenApp> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Vec<SeenApp>>(&text).ok())
        .unwrap_or_default()
}

/// Writes the app list through a temp file and a rename, like the settings file. Only ids and
/// names are written, never notification text.
fn save_apps(path: &Path, apps: &[SeenApp]) {
    if let Err(error) = write_apps(path, apps) {
        eprintln!("notchy: could not save notification apps: {error}");
    }
}

fn write_apps(path: &Path, apps: &[SeenApp]) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(apps).map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, json)?;
    fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Instants `ms` after a fixed start, so the tests never read the clock.
    struct Clock(Instant);

    impl Clock {
        fn new() -> Self {
            Self(Instant::now())
        }

        fn at(&self, ms: u64) -> Instant {
            self.0 + Duration::from_millis(ms)
        }
    }

    fn pending(title: &str) -> Pending {
        Pending {
            app: "Mail".to_string(),
            title: title.to_string(),
            body: String::new(),
            icon: None,
        }
    }

    fn titles(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|step| match step {
                Step::Show(notice) => Some(notice.title.clone()),
                Step::Hide => None,
            })
            .collect()
    }

    // ---- Tracker: dedupe and the silent first poll ----

    #[test]
    fn the_first_poll_records_what_is_there_and_shows_none_of_it() {
        let mut tracker = Tracker::default();
        assert!(tracker.fresh(&[10, 11, 12]).is_empty());
    }

    #[test]
    fn later_polls_show_only_new_ids_in_the_order_given() {
        let mut tracker = Tracker::default();
        tracker.fresh(&[10, 11]);
        assert_eq!(tracker.fresh(&[10, 11, 13, 12]), vec![2, 3]);
    }

    #[test]
    fn an_id_already_seen_is_not_shown_again() {
        let mut tracker = Tracker::default();
        tracker.fresh(&[10]);
        assert_eq!(tracker.fresh(&[11]), vec![0]);
        assert!(tracker.fresh(&[11]).is_empty());
        assert!(tracker.fresh(&[10, 11]).is_empty());
    }

    #[test]
    fn a_reset_silences_the_next_poll_again() {
        let mut tracker = Tracker::default();
        tracker.fresh(&[10]);
        tracker.reset();
        assert!(tracker.fresh(&[10, 11]).is_empty());
        assert_eq!(tracker.fresh(&[10, 11, 12]), vec![2]);
    }

    #[test]
    fn an_empty_list_keeps_the_ids_it_already_saw() {
        let mut tracker = Tracker::default();
        tracker.fresh(&[10, 11]);
        assert!(tracker.fresh(&[]).is_empty());
        // The listener returning an empty list for a moment does not replay the old toasts.
        assert!(tracker.fresh(&[10, 11]).is_empty());
    }

    #[test]
    fn the_oldest_seen_ids_are_forgotten_beyond_the_cap() {
        let mut tracker = Tracker::default();
        let ids: Vec<u32> = (1..=SEEN_CAP as u32).collect();
        tracker.fresh(&ids);
        assert_eq!(tracker.seen.len(), SEEN_CAP);
        tracker.fresh(&[SEEN_CAP as u32 + 1]);
        assert_eq!(tracker.seen.len(), SEEN_CAP);
        assert!(!tracker.seen.contains(&1));
        assert!(tracker.seen.contains(&(SEEN_CAP as u32 + 1)));
    }

    // ---- Queue: order, the give-way time, the cap and clearing ----

    #[test]
    fn the_first_notification_shows_at_once() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        let steps = queue.push(pending("one"), clock.at(0));
        assert_eq!(titles(&steps), vec!["one"]);
        assert_eq!(queue.next_deadline(), Some(clock.at(5000)));
    }

    #[test]
    fn a_newer_notification_waits_for_the_one_on_screen() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        queue.push(pending("one"), clock.at(0));
        assert!(queue.push(pending("two"), clock.at(100)).is_empty());
        // The first one keeps its full time: the second does not cut it short.
        assert!(queue.advance(clock.at(4999)).is_empty());
        assert_eq!(queue.next_deadline(), Some(clock.at(5000)));
    }

    #[test]
    fn the_queue_moves_on_after_the_show_time_in_arrival_order() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        queue.push(pending("one"), clock.at(0));
        queue.push(pending("two"), clock.at(100));
        queue.push(pending("three"), clock.at(200));

        assert_eq!(titles(&queue.advance(clock.at(5000))), vec!["two"]);
        assert_eq!(queue.next_deadline(), Some(clock.at(10_000)));
        assert_eq!(titles(&queue.advance(clock.at(10_000))), vec!["three"]);
    }

    #[test]
    fn the_island_hides_when_the_last_notification_ends() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        queue.push(pending("one"), clock.at(0));
        assert_eq!(queue.advance(clock.at(5000)), vec![Step::Hide]);
        assert_eq!(queue.next_deadline(), None);
        assert!(queue.advance(clock.at(9000)).is_empty());
    }

    #[test]
    fn the_queue_keeps_only_the_newest_waiting_notifications() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        queue.push(pending("on screen"), clock.at(0));
        // Seven more arrive while one is up. The two oldest are dropped.
        for name in ["b", "c", "d", "e", "f", "g", "h"] {
            queue.push(pending(name), clock.at(10));
        }
        assert_eq!(queue.waiting.len(), MAX_WAITING);

        let mut shown = Vec::new();
        let mut at = 5000;
        while queue.next_deadline().is_some() {
            shown.extend(titles(&queue.advance(clock.at(at))));
            at += 5000;
        }
        assert_eq!(shown, vec!["d", "e", "f", "g", "h"]);
    }

    #[test]
    fn sequence_numbers_count_every_notification_in_order() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        let first = queue.push(pending("one"), clock.at(0));
        let second = queue.push(pending("two"), clock.at(1));
        assert_eq!(first, vec![Step::Show(Notice { seq: 1, app: "Mail".into(), title: "one".into(), body: String::new(), icon: None })]);
        assert!(second.is_empty());
        let next = queue.advance(clock.at(5000));
        assert!(matches!(&next[0], Step::Show(notice) if notice.seq == 2));
    }

    #[test]
    fn clearing_drops_the_queue_and_hides_what_is_on_screen() {
        let clock = Clock::new();
        let mut queue = Queue::default();
        queue.push(pending("one"), clock.at(0));
        queue.push(pending("two"), clock.at(1));
        assert_eq!(queue.clear(), vec![Step::Hide]);
        assert_eq!(queue.next_deadline(), None);
        assert!(queue.advance(clock.at(5000)).is_empty());
    }

    #[test]
    fn clearing_an_empty_queue_does_nothing() {
        let mut queue = Queue::default();
        assert!(queue.clear().is_empty());
    }

    // ---- Text: tidying, truncation and the title and body ----

    #[test]
    fn tidy_collapses_whitespace_and_trims() {
        assert_eq!(tidy("  Meeting \n\t at  3 "), "Meeting at 3");
        assert_eq!(tidy("   "), "");
    }

    #[test]
    fn truncate_leaves_short_text_alone_and_ends_long_text_with_an_ellipsis() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("exactly ten", 11), "exactly ten");
        assert_eq!(truncate("a longer sentence", 8), "a longe…");
    }

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // Each of these is two bytes, so a byte cut would split one.
        assert_eq!(truncate("ééééé", 3), "éé…");
        assert_eq!(truncate("héllo wörld", 6), "héllo…");
    }

    #[test]
    fn truncate_does_not_leave_a_space_before_the_ellipsis() {
        assert_eq!(truncate("one two three", 5), "one…");
    }

    #[test]
    fn compose_splits_the_first_line_from_the_rest() {
        let lines = vec![
            " Alice  Chen ".to_string(),
            "Lunch at noon?".to_string(),
            "Bring the   notes".to_string(),
        ];
        assert_eq!(
            compose(&lines),
            Some((
                "Alice Chen".to_string(),
                "Lunch at noon? Bring the notes".to_string()
            ))
        );
    }

    #[test]
    fn compose_drops_blank_and_repeated_lines() {
        let lines = vec![
            "Build finished".to_string(),
            "   ".to_string(),
            "Build finished".to_string(),
            "All 42 tests passed".to_string(),
        ];
        assert_eq!(
            compose(&lines),
            Some(("Build finished".to_string(), "All 42 tests passed".to_string()))
        );
    }

    #[test]
    fn compose_with_one_line_has_no_body() {
        assert_eq!(
            compose(&["Only a title".to_string()]),
            Some(("Only a title".to_string(), String::new()))
        );
    }

    #[test]
    fn compose_gives_none_without_any_text() {
        assert_eq!(compose(&[]), None);
        assert_eq!(compose(&["".to_string(), " ".to_string()]), None);
    }

    #[test]
    fn compose_truncates_title_and_body_to_their_limits() {
        let (title, body) = compose(&["x".repeat(300), "y".repeat(300)]).unwrap();
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with('…'));
        assert_eq!(body.chars().count(), BODY_CHARS);
        assert!(body.ends_with('…'));
    }

    // ---- Filtering and the app list ----

    #[test]
    fn denied_apps_are_filtered_and_the_rest_are_shown() {
        let denied = vec!["Contoso.Mail_1234!App".to_string()];
        assert!(!allowed("Contoso.Mail_1234!App", &denied));
        assert!(allowed("Contoso.Chat_1234!App", &denied));
        assert!(allowed("Contoso.Mail_1234!App", &[]));
    }

    #[test]
    fn app_ids_match_without_case() {
        let denied = vec!["chrome".to_string()];
        assert!(!allowed("Chrome", &denied));
        assert!(!allowed("CHROME", &denied));
    }

    #[test]
    fn remember_adds_each_app_once_and_keeps_its_first_name() {
        let mut apps = Vec::new();
        assert!(remember(&mut apps, "Mail", "Mail app"));
        assert!(!remember(&mut apps, "Mail", "Renamed"));
        assert!(!remember(&mut apps, "mail", "Mail app"));
        assert_eq!(apps, vec![SeenApp { id: "Mail".into(), name: "Mail app".into(), icon: None }]);
    }

    #[test]
    fn remember_ignores_an_empty_id() {
        let mut apps = Vec::new();
        assert!(!remember(&mut apps, "", "Nameless"));
        assert!(apps.is_empty());
    }

    #[test]
    fn remember_forgets_the_oldest_app_beyond_the_cap() {
        let mut apps = Vec::new();
        for n in 0..=MAX_APPS {
            remember(&mut apps, &format!("app{n}"), "name");
        }
        assert_eq!(apps.len(), MAX_APPS);
        assert_eq!(apps[0].id, "app1");
    }

    #[test]
    fn the_snapshot_serializes_for_the_settings_window() {
        let json = serde_json::to_string(&Snapshot {
            access: Access::Unspecified,
            apps: vec![SeenApp { id: "a".into(), name: "A".into(), icon: None }],
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"access":"unspecified","apps":[{"id":"a","name":"A"}]}"#
        );
        assert_eq!(serde_json::to_string(&Access::Unknown).unwrap(), r#""unknown""#);
    }

    #[test]
    fn the_notice_serializes_for_the_island() {
        let json = serde_json::to_string(&Notice {
            seq: 3,
            app: "Mail".into(),
            title: "Hi".into(),
            body: "There".into(),
            icon: None,
        })
        .unwrap();
        assert_eq!(json, r#"{"seq":3,"app":"Mail","title":"Hi","body":"There"}"#);
    }

    #[test]
    fn a_logo_is_stored_once_per_app_and_read_back() {
        let mut apps = Vec::new();
        remember(&mut apps, "Mail", "Mail");
        assert!(needs_icon(&apps, "mail"));
        assert!(!needs_icon(&apps, "Unknown"));
        assert!(set_icon(&mut apps, "MAIL", "data:image/png;base64,AA==".into()));
        assert!(!set_icon(&mut apps, "Mail", "data:image/png;base64,AA==".into()));
        assert!(!set_icon(&mut apps, "Unknown", "x".into()));
        assert!(!needs_icon(&apps, "Mail"));
        assert_eq!(icon_of(&apps, "mail").as_deref(), Some("data:image/png;base64,AA=="));
        assert_eq!(icon_of(&apps, "Unknown"), None);
    }

    #[test]
    fn an_apps_file_from_before_logos_still_loads() {
        let apps: Vec<SeenApp> = serde_json::from_str(r#"[{"id":"Mail","name":"Mail"}]"#).unwrap();
        assert_eq!(apps[0].icon, None);
        let json = serde_json::to_string(&SeenApp { id: "a".into(), name: "A".into(), icon: Some("i".into()) }).unwrap();
        assert_eq!(json, r#"{"id":"a","name":"A","icon":"i"}"#);
    }

    #[test]
    fn the_app_list_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("notchy-notif-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(APPS_FILE);
        let apps = vec![SeenApp { id: "Mail".into(), name: "Mail".into(), icon: None }];
        write_apps(&path, &apps).unwrap();
        assert_eq!(load_apps(&path), apps);
        assert!(load_apps(&dir.join("missing.json")).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }
}
