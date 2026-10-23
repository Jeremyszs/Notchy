//! The built-in calendar. The person makes events in Settings, and Notchy reminds them a set time
//! before each start (`leadMinutes`, default 10) and at the start. An all-day event is announced at
//! `allDayHour` on its day. Nothing is fetched from the network.
//!
//! The pieces: `model` holds events, recurrence and the date math. `source` holds the `EventSource`
//! seam and the editable local source. `store` reads and writes the files. `service` merges the
//! sources. `reminders` decides what is due and what the pill shows. This file owns the scheduler
//! thread, the commands and the events the windows listen to.
//!
//! The reminder pill is a transient, like the notification pill. Its precedence is in `state.rs`.
//!
//! To add a read-only ICS source later:
//! 1. Add `calendar/ics.rs` with a struct that implements `EventSource`. Its `read_only` returns true,
//!    and its `series` returns the feed's events as `CalendarEvent`s. A feed that repeats by RRULE
//!    needs `Recurrence` extended first (see `model.rs`). Keep the fetched copy in memory and refresh
//!    it on a worker thread, so `series` never waits on the network.
//! 2. Add it to the sources in `start` below, with a stable id such as `ics:<hash of the URL>`.
//! 3. Nothing else changes. The scheduler, the island and the settings read the merged
//!    `EventInstance`s from `CalendarService`, and a feed's events are refused by `upsert`.

mod model;
mod reminders;
mod service;
mod source;
mod store;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter, Manager, State};
use time::{Date, Duration, Month, PrimitiveDateTime, Time};
use windows::Win32::System::SystemInformation::GetLocalTime;

use super::{Module, ModuleContext};
use crate::config::{self, ConfigFile};
use crate::state::{self, SharedIsland};
use crate::tray;
use model::{CalendarEvent, EventInput, EventInstance, LOCAL_SOURCE, check, day_range, new_id};
pub use reminders::RecentReminder;
use reminders::{Queue, Reminders, Step, minutes_until, triggers};
use service::CalendarService;
use source::LocalSource;

/// The most recent reminder, or null when it is snoozed or expires.
const RECENT_EVENT: &str = "calendar://recent";
/// One reminder on screen. Mirrors `CalendarNotice` in src/lib/calendar.ts.
const SHOW_EVENT: &str = "calendar://show";
/// The event list changed. Sent with the whole list, so the windows need no second read.
const UPDATE_EVENT: &str = "calendar://update";
/// Asks Settings to select a day and open its add form. The payload is the "YYYY-MM-DD" day.
const ADD_EVENT: &str = "calendar://add-note";
/// The longest the scheduler sleeps. It also wakes this often, so a PC that slept, or a clock change,
/// is caught within a minute.
const MAX_WAIT: StdDuration = StdDuration::from_secs(60);
/// The most events the island list asks for.
const MAX_UPCOMING: usize = 20;

/// The calendar's shared state. Commands edit the service under the lock, and the scheduler reads it.
pub struct Hub {
    service: Mutex<CalendarService>,
    reminders: Mutex<Reminders>,
    /// Wakes the scheduler after an edit or a settings change, so the next reminder is recomputed.
    wake: Sender<()>,
}

fn lock(hub: &Hub) -> MutexGuard<'_, CalendarService> {
    hub.service.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct CalendarModule;

impl Module for CalendarModule {
    fn name(&self) -> &'static str {
        "calendar"
    }

    fn start(&self, ctx: ModuleContext) {
        let dir = ctx.app.state::<ConfigFile>().0.clone();
        let events_path = dir.with_file_name(store::EVENTS_FILE);
        let fired_path = dir.with_file_name(store::FIRED_FILE);

        let service = CalendarService::new(vec![Box::new(LocalSource::open(events_path))]);
        let (wake, wakes) = mpsc::channel();
        let hub = Arc::new(Hub {
            service: Mutex::new(service),
            reminders: Mutex::new(Reminders::from_fired(store::load_fired(&fired_path))),
            wake,
        });
        ctx.app.manage(hub.clone());

        let app = ctx.app.clone();
        let island = ctx.island.clone();
        thread::spawn(move || run_scheduler(app, island, hub, wakes, fired_path));
    }
}

/// Called when the calendar settings change. Wakes the scheduler, so a new lead time or an off switch
/// takes effect at once.
pub fn reapply(app: &AppHandle) {
    if let Some(hub) = app.try_state::<Arc<Hub>>() {
        let _ = hub.wake.send(());
    }
}

/// Owns the reminder state, the pill queue and the island's calendar state. Sleeps until the next
/// reminder, the next give-way or the minute, whichever comes first.
fn run_scheduler(
    app: AppHandle,
    island: SharedIsland,
    hub: Arc<Hub>,
    wakes: Receiver<()>,
    fired_path: PathBuf,
) {
    let mut queue = Queue::default();
    loop {
        let wall = now_local();
        let instant = Instant::now();
        let settings = config::calendar(&app);
        let mut steps = Vec::new();
        let mut next = None;

        let mut reminders = hub.reminders.lock().unwrap_or_else(PoisonError::into_inner);
        let mut recent_changed = reminders.expire(wall);
        if settings.enabled {
            // Per-event lead times can reach a week ahead.
            let instances = lock(&hub).between(wall - Duration::days(1), wall + Duration::days(8));
            let due_list = triggers(&instances, settings.lead_minutes, settings.all_day_hour);
            let due = reminders.due(wall, &due_list);
            recent_changed |= !due.is_empty();
            for trigger in due {
                let minutes = minutes_until(trigger.start, wall);
                steps.extend(queue.push(&trigger, minutes, instant));
            }
            reminders.prune(wall);
            if reminders.take_changed()
                && let Err(error) = store::save_fired(&fired_path, reminders.fired())
            {
                eprintln!("notchy: could not save calendar reminders: {error}");
            }
            next = reminders.next(wall, &due_list);
        } else {
            steps.extend(queue.clear());
        }
        let expiry = reminders.expiry(wall);
        if recent_changed {
            let _ = app.emit(RECENT_EVENT, reminders.recent(wall));
        }
        drop(reminders);
        steps.extend(queue.advance(instant));

        for step in steps {
            match step {
                Step::Show(notice) => {
                    let _ = app.emit(SHOW_EVENT, &notice);
                    state::commit(&app, &island, |machine| machine.set_calendar(true));
                }
                Step::Hide => {
                    state::commit(&app, &island, |machine| machine.set_calendar(false));
                }
            }
        }

        let wait = [
            next.map(|at| std_duration(at - wall)),
            expiry.map(|at| std_duration(at - wall)),
            queue
                .next_deadline()
                .map(|at| at.saturating_duration_since(instant)),
            Some(MAX_WAIT),
        ]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(MAX_WAIT);
        match wakes.recv_timeout(wait) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Tells the windows the list changed, and wakes the scheduler for the new reminders.
fn changed(app: &AppHandle, hub: &Hub) {
    let events = lock(hub).events();
    let _ = app.emit(UPDATE_EVENT, events);
    let _ = hub.wake.send(());
}

#[tauri::command]
pub fn calendar_recent_reminder(hub: State<'_, Arc<Hub>>) -> Option<RecentReminder> {
    hub.reminders
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .recent(now_local())
}

#[tauri::command]
pub fn calendar_snooze(
    app: AppHandle,
    hub: State<'_, Arc<Hub>>,
    minutes: u32,
) -> Result<(), String> {
    if ![5, 10].contains(&minutes) {
        return Err("Choose a snooze of 5 or 10 minutes.".to_string());
    }
    if !config::calendar(&app).enabled {
        return Err("Turn on calendar reminders before snoozing.".to_string());
    }
    let mut reminders = hub.reminders.lock().unwrap_or_else(PoisonError::into_inner);
    if !reminders.snooze(minutes, now_local()) {
        return Err("There is no recent reminder to snooze.".to_string());
    }
    let _ = app.emit(RECENT_EVENT, Option::<RecentReminder>::None);
    let _ = hub.wake.send(());
    Ok(())
}

/// Every event from every source, in start order. The settings list reads it.
#[tauri::command]
pub fn calendar_list(hub: State<'_, Arc<Hub>>) -> Vec<CalendarEvent> {
    lock(&hub).events()
}

/// The next `n` occurrences from now, in start order. The island's Calendar page reads it.
#[tauri::command]
pub fn calendar_upcoming(hub: State<'_, Arc<Hub>>, n: usize) -> Vec<EventInstance> {
    lock(&hub).upcoming(now_local(), n.clamp(1, MAX_UPCOMING))
}

/// The occurrences that overlap the days from `from` to `to`, both "YYYY-MM-DD" and both included, in
/// start order. The month grid asks for the days it shows, so a repeating event colours every day it
/// falls on. The expansion is the same one the reminders use.
#[tauri::command]
pub fn calendar_between(
    hub: State<'_, Arc<Hub>>,
    from: String,
    to: String,
) -> Result<Vec<EventInstance>, String> {
    let (from, to) = day_range(&from, &to)?;
    Ok(lock(&hub).between(from, to))
}

/// Adds an event, or replaces the one named by `input.id`. The input supplies the lead times and
/// length, including clearing them. Returns the saved event.
#[tauri::command]
pub fn calendar_upsert(
    app: AppHandle,
    hub: State<'_, Arc<Hub>>,
    input: EventInput,
) -> Result<CalendarEvent, String> {
    let event = upsert(&mut lock(&hub), &input, now_ms())?;
    changed(&app, &hub);
    Ok(event)
}

fn upsert(
    service: &mut CalendarService,
    input: &EventInput,
    now: u64,
) -> Result<CalendarEvent, String> {
    let checked = check(input)?;
    let existing = match &input.id {
        Some(id) => Some(
            service
                .events()
                .into_iter()
                .find(|event| &event.id == id && event.source == LOCAL_SOURCE)
                .ok_or_else(|| "That event no longer exists.".to_string())?,
        ),
        None => None,
    };
    let event = CalendarEvent {
        id: input.id.clone().unwrap_or_else(|| new_id(now)),
        source: LOCAL_SOURCE.to_string(),
        title: checked.title,
        notes: checked.notes,
        start: checked.start,
        all_day: input.all_day,
        duration_minutes: checked.duration_minutes,
        recurrence: input.recurrence,
        remind_before_minutes: checked.remind_before_minutes,
        created_ms: existing.as_ref().map_or(now, |event| event.created_ms),
        updated_ms: now,
    };
    service.upsert(event.clone())?;
    Ok(event)
}

/// Removes an event the user made. The whole series goes, not one occurrence.
#[tauri::command]
pub fn calendar_delete(app: AppHandle, hub: State<'_, Arc<Hub>>, id: String) -> Result<(), String> {
    lock(&hub).remove(LOCAL_SOURCE, &id)?;
    changed(&app, &hub);
    Ok(())
}

/// The day the Settings calendar should open its add form on. Set by `settings_open_calendar` and taken
/// once by the Settings window when it loads, since a window built just now may not be listening yet.
/// The event below covers a window that is already open.
#[derive(Default)]
pub struct PendingAdd(Mutex<Option<String>>);

impl PendingAdd {
    fn set(&self, day: String) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(day);
    }

    fn take(&self) -> Option<String> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

/// Opens Settings on the calendar with `date` selected and its add form open. The island window cannot
/// take text, so the typing happens in Settings. The payload is the "YYYY-MM-DD" day.
#[tauri::command]
pub fn settings_open_calendar(
    app: AppHandle,
    pending: State<'_, PendingAdd>,
    date: String,
) -> Result<(), String> {
    day_range(&date, &date)?;
    pending.set(date.clone());
    tray::open_settings(&app);
    let _ = app.emit_to(tray::SETTINGS_LABEL, ADD_EVENT, date);
    Ok(())
}

/// The pending day, if the Settings window was asked for one before it loaded. Read once.
#[tauri::command]
pub fn calendar_take_add_date(pending: State<'_, PendingAdd>) -> Option<String> {
    pending.take()
}

/// The local wall clock, to the second. The one place the clock is read: everything else takes the
/// time as an argument, so the rules stay testable.
fn now_local() -> PrimitiveDateTime {
    let now = unsafe { GetLocalTime() };
    let month = Month::try_from(u8::try_from(now.wMonth).unwrap_or(1)).unwrap_or(Month::January);
    let date =
        Date::from_calendar_date(i32::from(now.wYear), month, now.wDay as u8).unwrap_or(Date::MIN);
    let time = Time::from_hms(now.wHour as u8, now.wMinute as u8, now.wSecond as u8)
        .unwrap_or(Time::MIDNIGHT);
    PrimitiveDateTime::new(date, time)
}

/// Milliseconds since the Unix epoch, for the created and updated stamps.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| u64::try_from(elapsed.as_millis()).ok())
        .unwrap_or(0)
}

/// A `time` duration as a std one. A negative span is already due, so it waits for nothing.
fn std_duration(span: Duration) -> StdDuration {
    StdDuration::from_millis(u64::try_from(span.whole_milliseconds()).unwrap_or(0))
}

#[cfg(test)]
mod pending_add_tests {
    use super::*;

    #[test]
    fn upsert_round_trips_authoritative_length_and_leads() {
        let dir = std::env::temp_dir().join(format!("notchy-upsert-{}", std::process::id()));
        let path = dir.join("events.json");
        let mut service = CalendarService::new(vec![Box::new(LocalSource::open(path.clone()))]);
        let mut input: EventInput = serde_json::from_str(
            r#"{
            "title":"Lunch","notes":"","start":"2026-10-23T10:00",
            "allDay":false,"recurrence":"none","durationMinutes":45,
            "remindBeforeMinutes":[30,5,30]
        }"#,
        )
        .unwrap();
        let saved = upsert(&mut service, &input, 1).unwrap();
        assert_eq!(saved.duration_minutes, Some(45));
        assert_eq!(saved.remind_before_minutes, Some(vec![5, 30]));
        let mut reopened = CalendarService::new(vec![Box::new(LocalSource::open(path.clone()))]);
        assert_eq!(reopened.events(), vec![saved.clone()]);
        input.id = Some(saved.id);
        input.duration_minutes = None;
        input.remind_before_minutes = Some(vec![]);
        let replaced = upsert(&mut reopened, &input, 2).unwrap();
        assert_eq!(replaced.duration_minutes, None);
        assert_eq!(replaced.remind_before_minutes, Some(vec![]));
        assert_eq!(replaced.created_ms, 1);
        assert_eq!(replaced.updated_ms, 2);
        let instances = reopened.between(
            time::macros::datetime!(2026-10-23 00:00),
            time::macros::datetime!(2026-10-24 00:00),
        );
        assert_eq!(triggers(&instances, 10, 9).len(), 1);
        assert_eq!(
            CalendarService::new(vec![Box::new(LocalSource::open(path))]).events(),
            vec![replaced]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_pending_day_is_taken_once() {
        let pending = PendingAdd::default();
        assert_eq!(pending.take(), None);
        pending.set("2026-10-23".to_string());
        assert_eq!(pending.take(), Some("2026-10-23".to_string()));
        assert_eq!(pending.take(), None);
    }

    #[test]
    fn a_newer_day_replaces_an_untaken_one() {
        let pending = PendingAdd::default();
        pending.set("2026-10-23".to_string());
        pending.set("2026-11-01".to_string());
        assert_eq!(pending.take(), Some("2026-11-01".to_string()));
    }
}
