//! Volume and brightness HUD. When the system volume, mute or panel brightness changes, the
//! island shows a slim HUD for a moment, then gives way again.
//!
//! Two sources feed one channel of `HudUpdate`s. `volume` takes Core Audio's endpoint callbacks,
//! so every change is seen whatever caused it. `brightness` takes WMI's brightness event. Nothing
//! polls. The `Hud` scheduler below drops repeats, coalesces key-repeat bursts, and holds the
//! transient island state for `HOLD` after the latest change. It is pure, so the timing is tested
//! without a clock or a window.
//!
//! Changes that Notchy makes itself, from the sliders in the expanded panel, are not shown.

mod brightness;
mod volume;

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use windows::core::GUID;

use super::{Module, ModuleContext};
use crate::config;
use crate::state::{self, SharedIsland};

const UPDATE_EVENT: &str = "hud://update";
/// How long the HUD stays after the last change. A new change restarts the wait.
const HOLD: Duration = Duration::from_millis(1600);
/// The shortest gap between emitted updates. Key repeats come faster than frames, so the newest
/// value goes out when the gap ends, and the bar always lands on the final level.
const EMIT_INTERVAL: Duration = Duration::from_millis(33);
/// A brightness change this soon after Notchy's own brightness write is that write, not a key.
const OWN_WRITE_WINDOW: Duration = Duration::from_millis(2000);

/// Event context that Notchy tags its own volume writes with. Core Audio echoes it on the
/// notification, so the HUD can tell the panel's slider from every other source.
pub const OWN_CONTEXT: GUID = GUID::from_u128(0x7d1b_6f2e_4c3a_4e8b_9f10_a5c2_d3e4_f607);

/// Brightness writes made by Notchy, as the time until which their echoes are ignored.
static OWN_BRIGHTNESS_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);

/// What the HUD shows. Mirrors the `hud://update` payload in src/stores/hud.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Volume,
    Brightness,
}

impl Kind {
    fn slot(self) -> usize {
        match self {
            Self::Volume => 0,
            Self::Brightness => 1,
        }
    }
}

/// One change. `level` is 0..=100. `muted` is only ever set for volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HudUpdate {
    pub kind: Kind,
    pub level: u8,
    pub muted: bool,
}

/// What the scheduler asks the thread to do. Emits come before the transient change they belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Emit(HudUpdate),
    /// Put the island into or take it out of the HUD state.
    Transient(bool),
}

/// The HUD scheduler. Every method takes the time, so the timing is testable.
#[derive(Debug, Default)]
pub struct Hud {
    /// The last change accepted for each kind. Repeating it is not a change.
    last: [Option<HudUpdate>; 2],
    /// The newest accepted change that has not been emitted yet.
    pending: Option<HudUpdate>,
    last_emit: Option<Instant>,
    /// When the HUD state ends, if no later change has pushed it back.
    hold_until: Option<Instant>,
    shown: bool,
}

impl Hud {
    /// Takes one change from a source. A repeat of the last value for that kind is dropped, and
    /// anything else restarts the hold.
    pub fn change(&mut self, update: HudUpdate, now: Instant) {
        let slot = &mut self.last[update.kind.slot()];
        if *slot == Some(update) {
            return;
        }
        *slot = Some(update);
        self.pending = Some(update);
        self.hold_until = Some(now + HOLD);
    }

    /// Returns what is due at `now`: the pending update, once the emit gap has passed, and the
    /// transient change when the hold starts or ends.
    pub fn advance(&mut self, now: Instant) -> Vec<Action> {
        let mut actions = Vec::new();

        if self.emit_due(now)
            && let Some(update) = self.pending.take()
        {
            actions.push(Action::Emit(update));
            self.last_emit = Some(now);
        }

        match self.hold_until {
            Some(until) if now >= until => {
                self.hold_until = None;
                if self.shown {
                    self.shown = false;
                    actions.push(Action::Transient(false));
                }
            }
            Some(_) if !self.shown => {
                self.shown = true;
                actions.push(Action::Transient(true));
            }
            _ => {}
        }

        actions
    }

    /// The next moment `advance` has work to do, or None when nothing is pending.
    pub fn next_deadline(&self) -> Option<Instant> {
        let emit = self
            .pending
            .and(self.last_emit.map(|at| at + EMIT_INTERVAL));
        emit.into_iter().chain(self.hold_until).min()
    }

    fn emit_due(&self, now: Instant) -> bool {
        self.pending.is_some()
            && self
                .last_emit
                .is_none_or(|at| now.saturating_duration_since(at) >= EMIT_INTERVAL)
    }
}

/// Decides whether a brightness reading is a change to show. `last` holds the level seen before
/// and is set to this reading either way, so an absorbed change still moves the baseline. With no
/// baseline yet, the first reading only sets it.
pub fn brightness_step(last: &mut Option<u8>, level: u8, own_write: bool) -> Option<u8> {
    let previous = last.replace(level);
    let changed = previous.is_some_and(|before| before != level);
    (changed && !own_write).then_some(level)
}

/// The HUD level for a Core Audio master volume scalar. NaN counts as silent.
pub fn level_from_scalar(scalar: f32) -> u8 {
    if scalar.is_nan() {
        0
    } else {
        (scalar * 100.0).round().clamp(0.0, 100.0) as u8
    }
}

/// Called before Notchy writes brightness itself, so the echo of that write is not a key press.
pub(crate) fn note_own_brightness_write() {
    let until = Instant::now() + OWN_WRITE_WINDOW;
    *OWN_BRIGHTNESS_UNTIL
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(until);
}

/// Whether `now` falls inside a window that Notchy's own brightness write opened.
fn within(until: Option<Instant>, now: Instant) -> bool {
    until.is_some_and(|until| now < until)
}

fn own_brightness_write_recent(now: Instant) -> bool {
    let until = *OWN_BRIGHTNESS_UNTIL
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    within(until, now)
}

pub struct HudModule;

impl Module for HudModule {
    fn name(&self) -> &'static str {
        "hud"
    }

    fn start(&self, ctx: ModuleContext) {
        let (updates, inbox) = mpsc::channel();
        let volume_updates = updates.clone();
        thread::spawn(move || volume::run(volume_updates));
        thread::spawn(move || brightness::run(updates));
        thread::spawn(move || run(ctx.app, ctx.island, inbox));
    }
}

/// Owns the scheduler. Waits for the next change or the next scheduled step, whichever is first.
fn run(app: AppHandle, island: SharedIsland, inbox: Receiver<HudUpdate>) {
    let mut hud = Hud::default();
    loop {
        let received = match hud.next_deadline() {
            Some(at) => inbox.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(update) => {
                if config::hud_enabled(&app) {
                    hud.change(update, Instant::now());
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }

        for action in hud.advance(Instant::now()) {
            match action {
                Action::Emit(update) => {
                    let _ = app.emit(UPDATE_EVENT, update);
                }
                Action::Transient(on) => {
                    state::commit(&app, &island, |machine| machine.set_transient(on));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(level: u8, muted: bool) -> HudUpdate {
        HudUpdate {
            kind: Kind::Volume,
            level,
            muted,
        }
    }

    fn brightness(level: u8) -> HudUpdate {
        HudUpdate {
            kind: Kind::Brightness,
            level,
            muted: false,
        }
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

    #[test]
    fn a_change_emits_and_shows_the_hud_at_once() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(40, false), clock.at(0));
        assert_eq!(
            hud.advance(clock.at(0)),
            vec![Action::Emit(volume(40, false)), Action::Transient(true)]
        );
    }

    #[test]
    fn the_hud_hides_after_the_hold_and_not_before() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(40, false), clock.at(0));
        hud.advance(clock.at(0));

        assert!(hud.advance(clock.at(1599)).is_empty());
        assert_eq!(hud.advance(clock.at(1600)), vec![Action::Transient(false)]);
        assert_eq!(hud.next_deadline(), None);
    }

    #[test]
    fn a_new_change_restarts_the_hold() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(40, false), clock.at(0));
        hud.advance(clock.at(0));

        hud.change(volume(41, false), clock.at(1000));
        assert_eq!(
            hud.advance(clock.at(1000)),
            vec![Action::Emit(volume(41, false))]
        );

        // The first hold would have ended at 1600 ms. The restarted one ends at 2600 ms.
        assert!(hud.advance(clock.at(1600)).is_empty());
        assert!(hud.advance(clock.at(2599)).is_empty());
        assert_eq!(hud.advance(clock.at(2600)), vec![Action::Transient(false)]);
    }

    #[test]
    fn a_repeat_of_the_last_value_is_not_a_change() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(40, false), clock.at(0));
        hud.advance(clock.at(0));

        hud.change(volume(40, false), clock.at(100));
        assert!(hud.advance(clock.at(100)).is_empty());
        // The hold was not pushed back by the repeat.
        assert_eq!(hud.next_deadline(), Some(clock.at(1600)));
    }

    #[test]
    fn mute_is_a_change_even_at_the_same_level() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(40, false), clock.at(0));
        hud.advance(clock.at(0));

        hud.change(volume(40, true), clock.at(50));
        assert_eq!(
            hud.advance(clock.at(50)),
            vec![Action::Emit(volume(40, true))]
        );
    }

    #[test]
    fn each_kind_keeps_its_own_repeat_check() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(50, false), clock.at(0));
        hud.advance(clock.at(0));

        hud.change(brightness(50), clock.at(100));
        assert_eq!(
            hud.advance(clock.at(100)),
            vec![Action::Emit(brightness(50))]
        );
    }

    #[test]
    fn a_burst_coalesces_to_the_newest_value_on_the_emit_gap() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        hud.change(volume(10, false), clock.at(0));
        hud.advance(clock.at(0));

        hud.change(volume(11, false), clock.at(5));
        assert!(hud.advance(clock.at(5)).is_empty());
        assert_eq!(hud.next_deadline(), Some(clock.at(33)));

        hud.change(volume(12, false), clock.at(10));
        assert!(hud.advance(clock.at(10)).is_empty());
        assert_eq!(hud.next_deadline(), Some(clock.at(33)));

        assert_eq!(
            hud.advance(clock.at(33)),
            vec![Action::Emit(volume(12, false))]
        );
        // The hold runs from the newest change, at 10 ms.
        assert_eq!(hud.next_deadline(), Some(clock.at(1610)));
    }

    #[test]
    fn next_deadline_is_the_earlier_of_the_emit_gap_and_the_hold() {
        let clock = Clock::new();
        let mut hud = Hud::default();
        assert_eq!(hud.next_deadline(), None);

        // Nothing has been emitted yet, so the only deadline is the hold.
        hud.change(volume(10, false), clock.at(0));
        assert_eq!(hud.next_deadline(), Some(clock.at(1600)));
        hud.advance(clock.at(0));

        // A change soon after an emit waits for the emit gap, which ends first.
        hud.change(volume(11, false), clock.at(10));
        assert_eq!(hud.next_deadline(), Some(clock.at(33)));
    }

    #[test]
    fn brightness_is_absorbed_until_there_is_a_baseline() {
        let mut last = None;
        assert_eq!(brightness_step(&mut last, 40, false), None);
        assert_eq!(last, Some(40));
    }

    #[test]
    fn brightness_reports_a_new_level_only() {
        let mut last = Some(40);
        assert_eq!(brightness_step(&mut last, 40, false), None);
        assert_eq!(brightness_step(&mut last, 45, false), Some(45));
        assert_eq!(last, Some(45));
    }

    #[test]
    fn own_brightness_writes_move_the_baseline_without_reporting() {
        let mut last = Some(40);
        assert_eq!(brightness_step(&mut last, 60, true), None);
        assert_eq!(last, Some(60));
        // The next key press is a real change again.
        assert_eq!(brightness_step(&mut last, 61, false), Some(61));
    }

    #[test]
    fn scalar_maps_to_a_clamped_percent() {
        assert_eq!(level_from_scalar(0.0), 0);
        assert_eq!(level_from_scalar(0.5), 50);
        assert_eq!(level_from_scalar(1.0), 100);
        assert_eq!(level_from_scalar(1.2), 100);
        assert_eq!(level_from_scalar(-0.3), 0);
        assert_eq!(level_from_scalar(f32::NAN), 0);
    }

    #[test]
    fn own_write_window_is_half_open() {
        let clock = Clock::new();
        let until = Some(clock.at(2000));
        assert!(within(until, clock.at(0)));
        assert!(within(until, clock.at(1999)));
        assert!(!within(until, clock.at(2000)));
        assert!(!within(None, clock.at(0)));
    }

    #[test]
    fn the_update_serializes_for_the_frontend() {
        let json = serde_json::to_string(&HudUpdate {
            kind: Kind::Brightness,
            level: 42,
            muted: false,
        })
        .unwrap();
        assert_eq!(json, r#"{"kind":"brightness","level":42,"muted":false}"#);
        let volume = serde_json::to_string(&volume(7, true)).unwrap();
        assert_eq!(volume, r#"{"kind":"volume","level":7,"muted":true}"#);
    }
}
