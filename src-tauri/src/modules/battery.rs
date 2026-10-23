//! Charger and battery alerts. A poller reads the system power status every `POLL_INTERVAL` on its
//! own thread. A `Tracker` turns each reading into the few moments worth a pill: the charger plugged
//! in or out, a full charge, and the low thresholds while on battery. A `Slot` holds the one pill on
//! screen for its time. Both are pure, so the rules are tested without a clock or Win32.
//!
//! The power status needs no COM or WinRT. A PC with no battery reads as `None` and shows nothing.
//! Nothing is written to disk.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use super::{Module, ModuleContext};
use crate::config::{self, Battery};
use crate::state::{self, SharedIsland};

/// One pill on screen. Mirrors the `battery://show` payload in src/lib/battery.ts.
const SHOW_EVENT: &str = "battery://show";
/// The latest reading, for the settings window. Sent only when it changes.
const STATUS_EVENT: &str = "battery://status";
/// How often the power status is read. A pill shows up this late at most.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// How long a charger or full-charge pill stays up. A newer pill replaces it and restarts the time.
pub const SHOW_FOR: Duration = Duration::from_secs(3);
/// How long a low-battery warning stays up. It asks for a read, so it is held longer.
pub const LOW_SHOW_FOR: Duration = Duration::from_secs(5);
/// The low-battery thresholds, in percent. Each fires once per discharge.
pub const LOW_AMBER: u8 = 20;
pub const LOW_RED: u8 = 10;
/// A threshold re-arms once the level rises this far above it.
pub const REARM_MARGIN: u8 = 3;
/// The `BatteryFlag` bit for a PC with no system battery. Windows also sets it when unknown.
const FLAG_NO_BATTERY: u8 = 128;
/// The `BatteryLifePercent` value when the level is not known.
const PERCENT_UNKNOWN: u8 = 255;
/// The `ACLineStatus` value when the charger is connected.
const AC_ONLINE: u8 = 1;

/// The battery as the power status reports it. `percent` is 0 to 100. Mirrors `BatteryReading` in
/// src/lib/battery.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
    pub percent: u8,
    pub plugged: bool,
}

/// Turns the raw power status into a reading. None when the PC has no battery, or the level is not
/// known yet.
pub fn reading(ac_line: u8, flag: u8, percent: u8) -> Option<Reading> {
    if flag & FLAG_NO_BATTERY != 0 || percent == PERCENT_UNKNOWN {
        return None;
    }
    Some(Reading {
        percent: percent.min(100),
        plugged: ac_line == AC_ONLINE,
    })
}

/// What a pill says. Mirrors the `kind` of `BatteryUpdate` in src/lib/battery.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Charging,
    Unplugged,
    Full,
    LowAmber,
    LowRed,
}

impl Kind {
    /// Charger and full-charge pills follow the charge alerts setting. Low-battery warnings follow
    /// the low battery setting.
    fn is_charge(self) -> bool {
        matches!(self, Self::Charging | Self::Unplugged | Self::Full)
    }

    fn hold(self) -> Duration {
        if self.is_charge() {
            SHOW_FOR
        } else {
            LOW_SHOW_FOR
        }
    }
}

/// One pill: what happened, and the level at that moment. Mirrors `BatteryUpdate` in
/// src/lib/battery.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub kind: Kind,
    pub percent: u8,
}

/// Whether a pill is wanted under the settings. Nothing shows while the feature is off.
pub fn wanted(update: Update, settings: &Battery) -> bool {
    let kind_on = if update.kind.is_charge() {
        settings.charge_alerts
    } else {
        settings.low_battery_alerts
    };
    settings.enabled && kind_on
}

/// Turns readings into the moments worth a pill. The first reading after a start, or after the
/// battery comes back, only records the state. Whatever is already true, such as a low level, is
/// marked as shown, so nothing fires at launch. Pure.
#[derive(Debug, Default)]
pub struct Tracker {
    last: Option<Reading>,
    amber_sent: bool,
    red_sent: bool,
    full_sent: bool,
}

impl Tracker {
    /// The pills due for this reading. A missing reading forgets everything, so the next one starts
    /// over silently.
    pub fn step(&mut self, reading: Option<Reading>) -> Vec<Update> {
        let Some(now) = reading else {
            *self = Self::default();
            return Vec::new();
        };
        let Some(before) = self.last.replace(now) else {
            self.amber_sent = !now.plugged && now.percent <= LOW_AMBER;
            self.red_sent = !now.plugged && now.percent <= LOW_RED;
            self.full_sent = now.plugged && now.percent >= 100;
            return Vec::new();
        };

        let update = |kind| Update {
            kind,
            percent: now.percent,
        };
        let mut updates = Vec::new();

        if now.plugged && !before.plugged {
            // A charger starts a new cycle, so both low thresholds re-arm.
            self.amber_sent = false;
            self.red_sent = false;
            if now.percent < 100 {
                updates.push(update(Kind::Charging));
            }
        }
        if !now.plugged && before.plugged {
            self.full_sent = false;
            updates.push(update(Kind::Unplugged));
        }

        if now.plugged {
            if now.percent >= 100 && !self.full_sent {
                self.full_sent = true;
                updates.push(update(Kind::Full));
            }
        } else {
            if now.percent > LOW_AMBER + REARM_MARGIN {
                self.amber_sent = false;
            }
            if now.percent > LOW_RED + REARM_MARGIN {
                self.red_sent = false;
            }
            // A drop straight past both thresholds shows only the lower one.
            if now.percent <= LOW_RED && !self.red_sent {
                self.red_sent = true;
                self.amber_sent = true;
                updates.push(update(Kind::LowRed));
            } else if now.percent <= LOW_AMBER && !self.amber_sent {
                self.amber_sent = true;
                updates.push(update(Kind::LowAmber));
            }
        }

        updates
    }
}

/// The one battery pill on screen, and when it gives way. A new pill replaces it at once and
/// restarts its time, so the latest event is the one the user sees. Pure.
#[derive(Debug, Default)]
pub struct Slot {
    showing: Option<(Update, Instant)>,
}

impl Slot {
    pub fn show(&mut self, update: Update, now: Instant) {
        self.showing = Some((update, now + update.kind.hold()));
    }

    /// True when the pill has had its time and gives way now.
    pub fn advance(&mut self, now: Instant) -> bool {
        match self.showing {
            Some((_, until)) if now >= until => {
                self.showing = None;
                true
            }
            _ => false,
        }
    }

    /// When the pill gives way, or None when nothing is on screen.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.showing.map(|(_, until)| until)
    }
}

/// The latest reading, for the settings window. Kept in memory only.
pub struct Latest {
    reading: Mutex<Option<Reading>>,
}

pub struct BatteryModule;

impl Module for BatteryModule {
    fn name(&self) -> &'static str {
        "battery"
    }

    fn start(&self, ctx: ModuleContext) {
        let latest = Arc::new(Latest {
            reading: Mutex::new(None),
        });
        ctx.app.manage(latest.clone());

        let (shows, inbox) = mpsc::channel();
        let scheduler_app = ctx.app.clone();
        let island = ctx.island.clone();
        thread::spawn(move || run_scheduler(scheduler_app, island, inbox));

        let poller_app = ctx.app.clone();
        thread::spawn(move || run_poller(poller_app, latest, shows));
    }
}

/// Reads the power status for the life of the app. Every reading goes to the tracker, whatever the
/// settings say, so the baseline stays true. Only the pills that are wanted are sent on.
fn run_poller(app: AppHandle, latest: Arc<Latest>, shows: Sender<Update>) {
    let mut tracker = Tracker::default();
    loop {
        let reading = read();
        publish(&app, &latest, reading);

        let settings = config::battery(&app);
        for update in tracker.step(reading) {
            if wanted(update, &settings) {
                let _ = shows.send(update);
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
}

/// One call to the power status. None when there is no battery, or the call fails.
fn read() -> Option<Reading> {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.ok()?;
    reading(
        status.ACLineStatus,
        status.BatteryFlag,
        status.BatteryLifePercent,
    )
}

/// Stores the reading, and tells the settings window when it has changed.
fn publish(app: &AppHandle, latest: &Latest, reading: Option<Reading>) {
    let changed = {
        let mut current = latest
            .reading
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if *current == reading {
            false
        } else {
            *current = reading;
            true
        }
    };
    if changed {
        let _ = app.emit(STATUS_EVENT, reading);
    }
}

/// Owns the pill slot and the island's battery state. Waits for the next pill or the give-way,
/// whichever comes first.
fn run_scheduler(app: AppHandle, island: SharedIsland, inbox: Receiver<Update>) {
    let mut slot = Slot::default();
    loop {
        let received = match slot.next_deadline() {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let now = Instant::now();
        match received {
            Ok(update) => {
                slot.show(update, now);
                let _ = app.emit(SHOW_EVENT, update);
                state::commit(&app, &island, |machine| machine.set_battery(true));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        if slot.advance(now) {
            state::commit(&app, &island, |machine| machine.set_battery(false));
        }
    }
}

/// The latest reading, or None on a PC with no battery. The settings window reads it on open.
#[tauri::command]
pub fn battery_snapshot(latest: State<'_, Arc<Latest>>) -> Option<Reading> {
    *latest
        .reading
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on_battery(percent: u8) -> Option<Reading> {
        Some(Reading {
            percent,
            plugged: false,
        })
    }

    fn plugged(percent: u8) -> Option<Reading> {
        Some(Reading {
            percent,
            plugged: true,
        })
    }

    fn kinds(updates: &[Update]) -> Vec<Kind> {
        updates.iter().map(|update| update.kind).collect()
    }

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

    // ---- Reading: what the power status means ----

    #[test]
    fn a_desktop_with_no_battery_reads_as_none() {
        assert_eq!(reading(0, FLAG_NO_BATTERY, PERCENT_UNKNOWN), None);
        assert_eq!(reading(1, 128, 100), None);
        assert_eq!(reading(255, 255, 255), None);
    }

    #[test]
    fn an_unknown_level_reads_as_none_but_a_known_one_does_not() {
        assert_eq!(reading(0, 1, 255), None);
        assert_eq!(
            reading(0, 2, 82),
            Some(Reading {
                percent: 82,
                plugged: false
            })
        );
    }

    #[test]
    fn only_a_connected_charger_counts_as_plugged_in() {
        assert_eq!(reading(1, 8, 40).map(|r| r.plugged), Some(true));
        assert_eq!(reading(0, 8, 40).map(|r| r.plugged), Some(false));
        // Unknown AC status is not a charger.
        assert_eq!(reading(255, 1, 40).map(|r| r.plugged), Some(false));
        assert_eq!(reading(0, 1, 140).map(|r| r.percent), Some(100));
    }

    // ---- Tracker: the first reading is silent ----

    #[test]
    fn the_first_reading_after_launch_is_silent_even_when_low() {
        let mut tracker = Tracker::default();
        assert!(tracker.step(on_battery(15)).is_empty());
        assert!(tracker.step(on_battery(15)).is_empty());
    }

    #[test]
    fn a_low_level_already_true_at_launch_is_not_announced_later_either() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(15));
        // Still 15 and falling, but the 20% warning was already true at launch.
        assert!(tracker.step(on_battery(14)).is_empty());
        // Only the lower threshold is still ahead.
        assert_eq!(kinds(&tracker.step(on_battery(10))), vec![Kind::LowRed]);
    }

    #[test]
    fn a_plugged_in_start_is_silent_and_an_unplug_is_announced() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(60));
        assert_eq!(kinds(&tracker.step(on_battery(60))), vec![Kind::Unplugged]);
    }

    // ---- Tracker: plug and unplug ----

    #[test]
    fn plugging_in_shows_charging_with_the_level() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(60));
        assert_eq!(
            tracker.step(plugged(61)),
            vec![Update {
                kind: Kind::Charging,
                percent: 61
            }]
        );
        // Still charging, nothing new.
        assert!(tracker.step(plugged(62)).is_empty());
    }

    #[test]
    fn unplugging_shows_the_level_once() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(60));
        assert_eq!(
            tracker.step(on_battery(60)),
            vec![Update {
                kind: Kind::Unplugged,
                percent: 60
            }]
        );
        assert!(tracker.step(on_battery(59)).is_empty());
    }

    #[test]
    fn plugging_in_at_full_shows_fully_charged_instead_of_charging() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(100));
        assert_eq!(kinds(&tracker.step(plugged(100))), vec![Kind::Full]);
    }

    // ---- Tracker: fully charged, once per charge ----

    #[test]
    fn reaching_full_while_plugged_fires_once() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(98));
        assert!(tracker.step(plugged(99)).is_empty());
        assert_eq!(kinds(&tracker.step(plugged(100))), vec![Kind::Full]);
        assert!(tracker.step(plugged(100)).is_empty());
        assert!(tracker.step(plugged(100)).is_empty());
    }

    #[test]
    fn full_fires_again_after_the_next_charge() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(98));
        tracker.step(plugged(100));
        tracker.step(on_battery(100));
        assert_eq!(kinds(&tracker.step(plugged(99))), vec![Kind::Charging]);
        assert_eq!(kinds(&tracker.step(plugged(100))), vec![Kind::Full]);
    }

    // ---- Tracker: low battery thresholds ----

    #[test]
    fn falling_to_twenty_warns_once() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        assert_eq!(
            tracker.step(on_battery(20)),
            vec![Update {
                kind: Kind::LowAmber,
                percent: 20
            }]
        );
        assert!(tracker.step(on_battery(19)).is_empty());
        assert!(tracker.step(on_battery(18)).is_empty());
    }

    #[test]
    fn falling_to_ten_warns_red_once_after_the_amber() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        assert_eq!(kinds(&tracker.step(on_battery(20))), vec![Kind::LowAmber]);
        assert_eq!(kinds(&tracker.step(on_battery(10))), vec![Kind::LowRed]);
        assert!(tracker.step(on_battery(9)).is_empty());
        assert!(tracker.step(on_battery(5)).is_empty());
    }

    #[test]
    fn a_drop_past_both_thresholds_shows_only_the_red_one() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        assert_eq!(
            tracker.step(on_battery(9)),
            vec![Update {
                kind: Kind::LowRed,
                percent: 9
            }]
        );
        assert!(tracker.step(on_battery(8)).is_empty());
    }

    /// Hysteresis: the amber warning re-arms only once the level is above 23 (20 + 3). At 23 it
    /// stays quiet, so a level hovering at the threshold does not warn on every dip.
    #[test]
    fn the_amber_warning_rearms_only_above_the_margin() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        assert_eq!(kinds(&tracker.step(on_battery(20))), vec![Kind::LowAmber]);

        assert!(tracker.step(on_battery(21)).is_empty());
        assert!(tracker.step(on_battery(23)).is_empty());
        assert!(tracker.step(on_battery(20)).is_empty());

        assert!(tracker.step(on_battery(24)).is_empty());
        assert_eq!(kinds(&tracker.step(on_battery(20))), vec![Kind::LowAmber]);
    }

    /// The red warning re-arms above 13 (10 + 3).
    #[test]
    fn the_red_warning_rearms_above_its_margin() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        tracker.step(on_battery(10));

        assert!(tracker.step(on_battery(13)).is_empty());
        assert!(tracker.step(on_battery(9)).is_empty());

        assert!(tracker.step(on_battery(14)).is_empty());
        assert_eq!(kinds(&tracker.step(on_battery(10))), vec![Kind::LowRed]);
    }

    #[test]
    fn charging_rearms_both_warnings_for_the_next_discharge() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        tracker.step(on_battery(20));
        tracker.step(on_battery(10));

        // Plugged in at 10 and unplugged again at 10: the unplug is announced and the low level
        // warns again, because the discharge is a new one.
        assert_eq!(kinds(&tracker.step(plugged(10))), vec![Kind::Charging]);
        assert_eq!(
            kinds(&tracker.step(on_battery(10))),
            vec![Kind::Unplugged, Kind::LowRed]
        );
    }

    #[test]
    fn no_low_warning_while_the_charger_is_connected() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(50));
        assert!(tracker.step(plugged(5)).is_empty());
    }

    #[test]
    fn plugged_in_below_the_threshold_then_unplugged_warns() {
        let mut tracker = Tracker::default();
        tracker.step(plugged(15));
        assert_eq!(
            kinds(&tracker.step(on_battery(15))),
            vec![Kind::Unplugged, Kind::LowAmber]
        );
    }

    // ---- Tracker: no battery ----

    #[test]
    fn a_missing_battery_forgets_the_state_and_the_next_reading_is_silent() {
        let mut tracker = Tracker::default();
        tracker.step(on_battery(25));
        tracker.step(on_battery(20));
        assert!(tracker.step(None).is_empty());

        // The battery comes back (a UPS, say) at 20: silent, and the level is not re-announced.
        assert!(tracker.step(on_battery(20)).is_empty());
        assert!(tracker.step(on_battery(19)).is_empty());
    }

    #[test]
    fn no_battery_never_produces_a_pill() {
        let mut tracker = Tracker::default();
        for _ in 0..5 {
            assert!(tracker.step(None).is_empty());
        }
    }

    // ---- Settings and the slot ----

    #[test]
    fn the_settings_decide_which_pills_show() {
        let all = Battery::default();
        let charge = Update {
            kind: Kind::Charging,
            percent: 50,
        };
        let low = Update {
            kind: Kind::LowRed,
            percent: 9,
        };
        assert!(wanted(charge, &all));
        assert!(wanted(low, &all));

        let no_charge = Battery {
            charge_alerts: false,
            ..all
        };
        assert!(!wanted(charge, &no_charge));
        assert!(wanted(low, &no_charge));

        let no_low = Battery {
            low_battery_alerts: false,
            ..all
        };
        assert!(wanted(charge, &no_low));
        assert!(!wanted(low, &no_low));

        let off = Battery {
            enabled: false,
            ..all
        };
        assert!(!wanted(charge, &off));
        assert!(!wanted(low, &off));
    }

    #[test]
    fn a_charge_pill_holds_three_seconds_and_a_low_one_five() {
        let clock = Clock::new();
        let mut slot = Slot::default();
        slot.show(
            Update {
                kind: Kind::Unplugged,
                percent: 80,
            },
            clock.at(0),
        );
        assert_eq!(slot.next_deadline(), Some(clock.at(3000)));
        assert!(!slot.advance(clock.at(2999)));
        assert!(slot.advance(clock.at(3000)));
        assert_eq!(slot.next_deadline(), None);

        slot.show(
            Update {
                kind: Kind::LowAmber,
                percent: 20,
            },
            clock.at(10_000),
        );
        assert_eq!(slot.next_deadline(), Some(clock.at(15_000)));
    }

    /// A newer pill takes the slot and restarts the time, so the latest one is the one seen.
    #[test]
    fn a_newer_pill_replaces_the_one_on_screen_and_restarts_its_time() {
        let clock = Clock::new();
        let mut slot = Slot::default();
        slot.show(
            Update {
                kind: Kind::Charging,
                percent: 40,
            },
            clock.at(0),
        );
        slot.show(
            Update {
                kind: Kind::LowAmber,
                percent: 20,
            },
            clock.at(2000),
        );
        assert!(!slot.advance(clock.at(3000)));
        assert_eq!(slot.next_deadline(), Some(clock.at(7000)));
    }

    #[test]
    fn the_pill_payloads_serialize_for_the_frontend() {
        let json = serde_json::to_string(&Update {
            kind: Kind::LowAmber,
            percent: 20,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"lowAmber","percent":20}"#);
        let reading = serde_json::to_string(&Reading {
            percent: 82,
            plugged: true,
        })
        .unwrap();
        assert_eq!(reading, r#"{"percent":82,"plugged":true}"#);
    }
}
