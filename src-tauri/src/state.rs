//! Island state machine. Hover, media playback, screen presence, transient overlays and live
//! activities feed in; the resolved state is emitted to the frontend and read by the cursor
//! watcher for hit testing.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandState {
    #[default]
    Idle,
    Active,
    /// A live activity, such as a running timer, owns the compact pill. Media does not show here.
    Activity,
    Expanded,
    /// Hovered while the media page has synced lyrics. A wider Expanded with the lyric stack.
    Lyrics,
    /// A foreground window sits under the notch. Shrunk to a sliver so it stays out of the way.
    Tucked,
    /// A transient overlay is up, such as the volume and brightness HUD. Slim, and gone after a
    /// moment. Its content travels on its own event; this state only says it is on screen.
    Hud,
    /// Another app's toast notification, shown for a few seconds. A transient like the HUD. Its
    /// content travels on `notifications://show`; this state only says it is on screen.
    Notification,
    /// A charger plugged in or out, a full charge, or a low battery. A transient like the HUD, shown
    /// above a notification. Its content travels on `battery://show`; this state only says it is on.
    Battery,
    /// A reminder for a calendar event, shown for a few seconds. A transient like the battery pill.
    /// Its content travels on `calendar://show`; this state only says it is on screen.
    Calendar,
    /// An activity wants attention, such as a timer that just ran out. A card that holds for a
    /// few seconds, and stays until its owner clears the alert. Its content travels on the
    /// owner's event.
    Alert,
    /// A fullscreen app owns the monitor. Invisible and never hit.
    Hidden,
}

impl IslandState {
    /// Logical pill size. Mirrors SIZES in src/Island.svelte; keep them in sync.
    pub fn pill_size(self) -> (f64, f64) {
        match self {
            Self::Idle => (120.0, 30.0),
            Self::Active => (200.0, 30.0),
            Self::Activity => (220.0, 30.0),
            Self::Expanded => (380.0, 192.0),
            Self::Lyrics => (620.0, 224.0),
            Self::Hud => (260.0, 30.0),
            Self::Notification => (260.0, 30.0),
            Self::Battery => (260.0, 30.0),
            Self::Calendar => (260.0, 30.0),
            Self::Alert => (320.0, 64.0),
            Self::Tucked => (64.0, 5.0),
            Self::Hidden => (0.0, 0.0),
        }
    }
}

/// Which module owns a live activity or an alert. The island only needs the kind to know that
/// something is on; the owner's own event carries the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityKind {
    Timer,
}

/// The inputs to `resolve`. Each flag is a fact the island reacts to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inputs {
    pub hovered: bool,
    pub media_playing: bool,
    pub fullscreen: bool,
    pub covered: bool,
    pub wide: bool,
    pub transient: bool,
    pub battery: bool,
    pub calendar: bool,
    pub notification: bool,
    pub activity: bool,
    pub alert: bool,
}

/// Precedence, highest first:
///
/// 1. Fullscreen hides everything.
/// 2. An alert card shows, even over the pointer. It holds for a few seconds, and the pointer
///    cannot push it aside, so its buttons stay where they are. A timer alert outranks a
///    notification, so an incoming message never hides a finished countdown.
/// 3. Hover expands (to the lyrics layout when it is wide).
/// 4. A transient overlay (the HUD) shows over the rest.
/// 5. A battery pill (charger, full, low battery) shows under the HUD and over a notification. It
///    is a short, physical event the user just caused, so it is not left waiting behind a message.
///    A notification that lands meanwhile keeps its own timer and shows once the pill gives way.
/// 6. A calendar reminder shows under the battery pill and over a notification. It is a reminder
///    the user asked for, so a toast from another app never hides it. It holds for 6 to 8 s, and a
///    reminder that lands meanwhile waits its turn in its own queue.
/// 7. A notification shows under the HUD, so a volume or brightness change is never hidden by a
///    message. Like the HUD, it shows over a covered notch.
/// 8. A covered notch tucks, even while an activity runs.
/// 9. A live activity owns the compact pill, in place of media.
/// 10. Media shows the active pill, and otherwise the island idles.
pub fn resolve(inputs: Inputs) -> IslandState {
    if inputs.fullscreen {
        IslandState::Hidden
    } else if inputs.alert {
        IslandState::Alert
    } else if inputs.hovered && inputs.wide {
        IslandState::Lyrics
    } else if inputs.hovered {
        IslandState::Expanded
    } else if inputs.transient {
        IslandState::Hud
    } else if inputs.battery {
        IslandState::Battery
    } else if inputs.calendar {
        IslandState::Calendar
    } else if inputs.notification {
        IslandState::Notification
    } else if inputs.covered {
        IslandState::Tucked
    } else if inputs.activity {
        IslandState::Activity
    } else if inputs.media_playing {
        IslandState::Active
    } else {
        IslandState::Idle
    }
}

#[derive(Debug, Default)]
pub struct IslandStateMachine {
    hovered: bool,
    media_playing: bool,
    fullscreen: bool,
    covered: bool,
    wide: bool,
    transient: bool,
    battery: bool,
    calendar: bool,
    notification: bool,
    activity: Option<ActivityKind>,
    alert: Option<ActivityKind>,
}

impl IslandStateMachine {
    pub fn current(&self) -> IslandState {
        resolve(Inputs {
            hovered: self.hovered,
            media_playing: self.media_playing,
            fullscreen: self.fullscreen,
            covered: self.covered,
            wide: self.wide,
            transient: self.transient,
            battery: self.battery,
            calendar: self.calendar,
            notification: self.notification,
            activity: self.activity.is_some(),
            alert: self.alert.is_some(),
        })
    }

    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    pub fn set_media_playing(&mut self, playing: bool) {
        self.media_playing = playing;
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
    }

    pub fn set_covered(&mut self, covered: bool) {
        self.covered = covered;
    }

    pub fn set_wide(&mut self, wide: bool) {
        self.wide = wide;
    }

    /// Whether a transient overlay (the HUD) is on screen. Owned by `modules::hud`.
    pub fn set_transient(&mut self, transient: bool) {
        self.transient = transient;
    }

    /// Whether a battery pill is on screen. Owned by `modules::battery`.
    pub fn set_battery(&mut self, battery: bool) {
        self.battery = battery;
    }

    /// Whether a calendar reminder is on screen. Owned by `modules::calendar`.
    pub fn set_calendar(&mut self, calendar: bool) {
        self.calendar = calendar;
    }

    /// Whether a notification is on screen. Owned by `modules::notifications`.
    pub fn set_notification(&mut self, notification: bool) {
        self.notification = notification;
    }

    /// The live activity that owns the compact pill, or None. Owned by its module.
    pub fn set_activity(&mut self, activity: Option<ActivityKind>) {
        self.activity = activity;
    }

    /// The alert card, or None. Owned by the module that raised it.
    pub fn set_alert(&mut self, alert: Option<ActivityKind>) {
        self.alert = alert;
    }
}

pub type SharedIsland = Arc<Mutex<IslandStateMachine>>;

pub fn lock(island: &SharedIsland) -> MutexGuard<'_, IslandStateMachine> {
    // The machine has no invariant a panic could break, so a poisoned lock is still safe to use.
    island.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Applies a change under the lock and emits the new state only if it differs.
/// Emitting after the guard drops keeps the cursor watcher off the event path.
pub fn commit(
    app: &AppHandle,
    island: &SharedIsland,
    change: impl FnOnce(&mut IslandStateMachine),
) {
    let changed = {
        let mut machine = lock(island);
        let before = machine.current();
        change(&mut machine);
        let after = machine.current();
        (after != before).then_some(after)
    };

    if let Some(state) = changed {
        let _ = app.emit("island://state", state);
    }
}

/// Tells the machine whether the media page has synced lyrics to show. Hover still decides
/// whether the wide layout appears, so leaving the island needs no help from the frontend.
#[tauri::command]
pub fn island_set_wide(app: AppHandle, island: State<'_, SharedIsland>, wide: bool) {
    commit(&app, &island, |machine| machine.set_wide(wide));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each precedence rung as the inputs that trigger it on their own, and the state it gives.
    /// Listed highest first, the same order as `resolve`.
    fn rungs() -> [(Inputs, IslandState); 11] {
        use IslandState::*;
        let base = Inputs::default();
        [
            (Inputs { fullscreen: true, ..base }, Hidden),
            (Inputs { alert: true, ..base }, Alert),
            (
                Inputs { hovered: true, wide: true, ..base },
                Lyrics,
            ),
            (Inputs { hovered: true, ..base }, Expanded),
            (Inputs { transient: true, ..base }, Hud),
            (Inputs { battery: true, ..base }, Battery),
            (Inputs { calendar: true, ..base }, Calendar),
            (Inputs { notification: true, ..base }, Notification),
            (Inputs { covered: true, ..base }, Tucked),
            (Inputs { activity: true, ..base }, Activity),
            (Inputs { media_playing: true, ..base }, Active),
        ]
    }

    fn union(a: Inputs, b: Inputs) -> Inputs {
        Inputs {
            hovered: a.hovered || b.hovered,
            media_playing: a.media_playing || b.media_playing,
            fullscreen: a.fullscreen || b.fullscreen,
            covered: a.covered || b.covered,
            wide: a.wide || b.wide,
            transient: a.transient || b.transient,
            battery: a.battery || b.battery,
            calendar: a.calendar || b.calendar,
            notification: a.notification || b.notification,
            activity: a.activity || b.activity,
            alert: a.alert || b.alert,
        }
    }

    #[test]
    fn each_rung_alone_gives_its_state_and_nothing_is_idle_by_accident() {
        assert_eq!(resolve(Inputs::default()), IslandState::Idle);
        for (inputs, expected) in rungs() {
            assert_eq!(resolve(inputs), expected, "{inputs:?}");
        }
    }

    /// Whenever two rungs are on together, the higher one wins. Checks all 55 pairs.
    #[test]
    fn a_higher_rung_always_beats_a_lower_one() {
        let rungs = rungs();
        for (i, (high, expected)) in rungs.iter().enumerate() {
            for (low, _) in &rungs[i + 1..] {
                assert_eq!(
                    resolve(union(*high, *low)),
                    *expected,
                    "high {high:?} against low {low:?}"
                );
            }
        }
    }

    #[test]
    fn machine_tracks_inputs() {
        let mut machine = IslandStateMachine::default();
        assert_eq!(machine.current(), IslandState::Idle);

        machine.set_media_playing(true);
        assert_eq!(machine.current(), IslandState::Active);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_media_playing(false);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Idle);
    }

    #[test]
    fn wide_shows_lyrics_only_while_hovered() {
        let mut machine = IslandStateMachine::default();
        machine.set_media_playing(true);
        machine.set_wide(true);
        assert_eq!(machine.current(), IslandState::Active);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Lyrics);

        machine.set_wide(false);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_wide(true);
        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Active);
    }

    #[test]
    fn covered_tucks_until_something_takes_priority() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        assert_eq!(machine.current(), IslandState::Tucked);

        machine.set_media_playing(true);
        assert_eq!(machine.current(), IslandState::Tucked);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);

        machine.set_fullscreen(false);
        machine.set_hovered(false);
        machine.set_covered(false);
        assert_eq!(machine.current(), IslandState::Active);
    }

    /// A HUD shows over a tucked or playing island, then gives way when it ends.
    #[test]
    fn transient_overlay_shows_over_tucked_and_active_then_returns() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        machine.set_media_playing(true);
        machine.set_transient(true);
        assert_eq!(machine.current(), IslandState::Hud);

        machine.set_transient(false);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// Hover and fullscreen both outrank the HUD.
    #[test]
    fn hover_and_fullscreen_outrank_the_transient_overlay() {
        let mut machine = IslandStateMachine::default();
        machine.set_transient(true);
        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_wide(true);
        assert_eq!(machine.current(), IslandState::Lyrics);

        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);

        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Hidden);
    }

    #[test]
    fn fullscreen_hides_regardless_of_other_inputs() {
        let mut machine = IslandStateMachine::default();
        machine.set_hovered(true);
        machine.set_media_playing(true);
        machine.set_covered(true);
        machine.set_wide(true);
        machine.set_transient(true);
        machine.set_battery(true);
        machine.set_calendar(true);
        machine.set_notification(true);
        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_alert(Some(ActivityKind::Timer));
        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);
    }

    /// A calendar reminder shows over a tucked or playing island and gives way when it ends.
    #[test]
    fn calendar_shows_over_tucked_and_active_then_returns() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        machine.set_media_playing(true);
        machine.set_calendar(true);
        assert_eq!(machine.current(), IslandState::Calendar);

        machine.set_calendar(false);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// The reminder sits under the battery pill, the HUD and the alert, and over a notification.
    /// Hover wins over it.
    #[test]
    fn calendar_yields_to_alert_hud_battery_and_hover_but_beats_a_notification() {
        let mut machine = IslandStateMachine::default();
        machine.set_notification(true);
        machine.set_calendar(true);
        assert_eq!(machine.current(), IslandState::Calendar);

        machine.set_battery(true);
        assert_eq!(machine.current(), IslandState::Battery);
        machine.set_battery(false);

        machine.set_transient(true);
        assert_eq!(machine.current(), IslandState::Hud);
        machine.set_transient(false);

        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);
        machine.set_alert(None);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);
        machine.set_hovered(false);

        machine.set_calendar(false);
        assert_eq!(machine.current(), IslandState::Notification);
    }

    /// A timer alert outranks a reminder, so a finished countdown is never hidden by one.
    #[test]
    fn a_timer_alert_outranks_a_calendar_reminder() {
        let mut machine = IslandStateMachine::default();
        machine.set_calendar(true);
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);
    }

    #[test]
    fn calendar_pill_is_compact_and_fits_the_window() {
        assert_eq!(IslandState::Calendar.pill_size(), (260.0, 30.0));
        let (w, h) = IslandState::Calendar.pill_size();
        assert!(w <= 720.0 && h <= 240.0);
    }

    /// A notification shows over a tucked or playing island and gives way when it ends, as the HUD does.
    #[test]
    fn notification_shows_over_tucked_and_active_then_returns() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        machine.set_media_playing(true);
        machine.set_notification(true);
        assert_eq!(machine.current(), IslandState::Notification);

        machine.set_notification(false);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// A timer alert and the HUD outrank a notification, and hover outranks it too.
    #[test]
    fn alert_and_hud_outrank_a_notification_and_hover_outranks_it() {
        let mut machine = IslandStateMachine::default();
        machine.set_notification(true);
        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);

        machine.set_alert(None);
        assert_eq!(machine.current(), IslandState::Notification);

        machine.set_transient(true);
        assert_eq!(machine.current(), IslandState::Hud);

        machine.set_transient(false);
        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);
    }

    /// A battery pill shows over a tucked or playing island and gives way when it ends.
    #[test]
    fn battery_shows_over_tucked_and_active_then_returns() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        machine.set_media_playing(true);
        machine.set_battery(true);
        assert_eq!(machine.current(), IslandState::Battery);

        machine.set_battery(false);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// The battery pill sits under the HUD and the alert, over a notification, and hover wins.
    #[test]
    fn battery_yields_to_alert_hud_and_hover_but_beats_a_notification() {
        let mut machine = IslandStateMachine::default();
        machine.set_notification(true);
        machine.set_battery(true);
        assert_eq!(machine.current(), IslandState::Battery);

        machine.set_transient(true);
        assert_eq!(machine.current(), IslandState::Hud);
        machine.set_transient(false);

        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);
        machine.set_alert(None);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);
        machine.set_hovered(false);

        machine.set_battery(false);
        assert_eq!(machine.current(), IslandState::Notification);
    }

    #[test]
    fn battery_pill_is_compact_and_fits_the_window() {
        assert_eq!(IslandState::Battery.pill_size(), (260.0, 30.0));
        let (w, h) = IslandState::Battery.pill_size();
        assert!(w <= 720.0 && h <= 240.0);
    }

    #[test]
    fn notification_pill_is_compact_and_fits_the_window() {
        assert_eq!(IslandState::Notification.pill_size(), (260.0, 30.0));
        let (w, h) = IslandState::Notification.pill_size();
        assert!(w <= 720.0 && h <= 240.0);
    }

    /// A running timer takes the compact pill from the media pill, and gives it back when it ends.
    #[test]
    fn activity_takes_the_compact_pill_from_media() {
        let mut machine = IslandStateMachine::default();
        machine.set_media_playing(true);
        machine.set_activity(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Activity);

        machine.set_activity(None);
        assert_eq!(machine.current(), IslandState::Active);
    }

    #[test]
    fn hover_expands_the_activity_pill_and_a_covering_window_tucks_it() {
        let mut machine = IslandStateMachine::default();
        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_hovered(false);
        machine.set_covered(true);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// The alert card holds over hover, so its buttons stay put, and it still gives way to
    /// fullscreen. It shows over a tucked notch, as the HUD does.
    #[test]
    fn alert_outranks_hover_and_the_hud_but_not_fullscreen() {
        let mut machine = IslandStateMachine::default();
        machine.set_hovered(true);
        machine.set_transient(true);
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);

        machine.set_covered(true);
        assert_eq!(machine.current(), IslandState::Alert);

        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);
    }

    #[test]
    fn clearing_the_alert_returns_to_the_activity_or_hover() {
        let mut machine = IslandStateMachine::default();
        machine.set_activity(Some(ActivityKind::Timer));
        machine.set_alert(Some(ActivityKind::Timer));
        assert_eq!(machine.current(), IslandState::Alert);

        machine.set_alert(None);
        assert_eq!(machine.current(), IslandState::Activity);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);
    }

    /// The window is 240 px tall (tauri.conf.json). The expanded pill, with its pager row, must
    /// leave room for it.
    #[test]
    fn expanded_pill_fits_the_window() {
        let (_, h) = IslandState::Expanded.pill_size();
        assert!(h <= 240.0, "expanded pill is {h} px tall");
    }

    /// The window is 720 px wide and 240 px tall (tauri.conf.json), so the lyrics pill must stay
    /// inside it, with room left for the outline's bleed above the top edge.
    #[test]
    fn lyrics_pill_fits_the_window() {
        let (w, h) = IslandState::Lyrics.pill_size();
        assert_eq!((w, h), (620.0, 224.0));
        assert!(w <= 720.0, "lyrics pill is {w} px wide");
        assert!(h <= 240.0, "lyrics pill is {h} px tall");
    }

    #[test]
    fn hud_pill_is_slim_and_fits_the_window() {
        let (w, h) = IslandState::Hud.pill_size();
        assert_eq!((w, h), (260.0, 30.0));
        assert!(w <= 720.0 && h <= 240.0);
    }

    #[test]
    fn activity_and_alert_fit_the_window() {
        assert_eq!(IslandState::Activity.pill_size(), (220.0, 30.0));
        assert_eq!(IslandState::Alert.pill_size(), (320.0, 64.0));
        for state in [IslandState::Activity, IslandState::Alert] {
            let (w, h) = state.pill_size();
            assert!(w <= 720.0 && h <= 240.0, "{state:?} is {w} x {h}");
        }
    }

    #[test]
    fn hidden_has_no_size() {
        assert_eq!(IslandState::Hidden.pill_size(), (0.0, 0.0));
        assert_eq!(IslandState::Tucked.pill_size(), (64.0, 5.0));
    }
}
