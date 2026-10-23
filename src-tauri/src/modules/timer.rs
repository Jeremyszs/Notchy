//! Timer, stopwatch and pomodoro. The clock lives here, in Rust, so a running timer keeps going
//! across page changes, a hidden notch and a closed settings window.
//!
//! `Engine` is pure. Every method takes the instant to act at, so the tests drive it with fixed
//! times and never read the clock. The worker thread sleeps on a condition variable until the next
//! deadline (a countdown reaching zero, or the alert card giving way), so nothing polls. Commands
//! change the engine under the same lock and wake the worker. The frontend extrapolates the digits
//! from `endsAtMs` between updates, so a tick is never sent.
//!
//! While a timer is running, paused or done, the island shows it as a live activity. When a
//! countdown ends, the island shows an alert card for `ALERT_HOLD`, and the sound plays once.

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;

use super::{Module, ModuleContext};
use crate::config::{self, Config, SharedConfig};
use crate::state::{self, ActivityKind, SharedIsland};

/// Full snapshot of the timer, sent after every change and every finish.
pub const UPDATE_EVENT: &str = "timer://update";
/// One event per countdown that ended, with the phase that ended and the one that started.
pub const FINISHED_EVENT: &str = "timer://finished";
/// How long the alert card holds before it gives way to the compact pill.
pub const ALERT_HOLD: Duration = Duration::from_millis(4000);
/// Laps kept per stopwatch run. Further laps are ignored.
pub const MAX_LAPS: usize = 20;
/// The longest countdown (3 h). Also the cap when time is added.
pub const MAX_COUNTDOWN_MS: u64 = 180 * 60_000;
/// The longest countdown the timer accepts, in minutes.
pub const MAX_TIMER_MINUTES: u32 = 180;
/// The timer's goal before the user picks one.
const DEFAULT_TIMER_MS: u64 = 5 * 60_000;
/// Focus sessions in one pomodoro cycle. A long break follows the last one.
pub const POMODORO_CYCLE: u8 = 4;

/// Which clock the notch runs. One runs at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Timer,
    Stopwatch,
    Pomodoro,
}

/// The part of a pomodoro cycle that is running. Plain timers and the stopwatch are always idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Focus,
    Break,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Idle,
    Running,
    Paused,
    /// A countdown reached zero and is waiting for the user to dismiss it. Timer mode only.
    Done,
}

/// Pomodoro phase lengths, in ms. Read from the config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Durations {
    pub focus_ms: u64,
    pub short_ms: u64,
    pub long_ms: u64,
}

impl Durations {
    pub fn from_minutes(focus: u32, short: u32, long: u32) -> Self {
        Self {
            focus_ms: minutes_to_ms(focus),
            short_ms: minutes_to_ms(short),
            long_ms: minutes_to_ms(long),
        }
    }

    fn for_phase(self, phase: Phase) -> u64 {
        match phase {
            Phase::Focus | Phase::Idle => self.focus_ms,
            Phase::Break => self.short_ms,
            Phase::Long => self.long_ms,
        }
    }
}

impl Default for Durations {
    fn default() -> Self {
        Self::from_minutes(
            config::FOCUS_MINUTES.0,
            config::SHORT_BREAK_MINUTES.0,
            config::LONG_BREAK_MINUTES.0,
        )
    }
}

/// One stopwatch lap. `total_ms` is the elapsed time at the lap, `split_ms` the time since the lap
/// before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Lap {
    pub total_ms: u64,
    pub split_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PomodoroView {
    /// The focus session in progress, or the one a break follows. 1 to `of`.
    pub session: u8,
    pub of: u8,
    /// Focus sessions finished in this cycle, for the dots.
    pub completed: u8,
}

/// What the frontend shows. Mirrors `TimerSnapshot` in src/stores/timer.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub mode: Mode,
    pub phase: Phase,
    pub status: Status,
    pub running: bool,
    /// The countdown length. Zero for the stopwatch.
    pub total_ms: u64,
    pub elapsed_ms: u64,
    /// Time left on the countdown. Zero for the stopwatch.
    pub remaining_ms: u64,
    /// Unix ms when a running countdown reaches zero. Null otherwise.
    pub ends_at_ms: Option<u64>,
    pub laps: Vec<Lap>,
    pub pomodoro: PomodoroView,
}

/// A countdown that ended during this change. The payload of `timer://finished`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finished {
    pub mode: Mode,
    /// The phase that ended. Idle for a plain timer.
    pub phase: Phase,
    /// The phase that started in its place. Pomodoro only.
    pub next: Option<Phase>,
    pub total_ms: u64,
}

/// What a `tick` changed.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tick {
    pub finished: Vec<Finished>,
    pub changed: bool,
}

/// The timer state machine. Every method that depends on time takes `now`.
#[derive(Debug, Clone)]
pub struct Engine {
    mode: Mode,
    status: Status,
    phase: Phase,
    /// The countdown length now: the timer's goal, or the current pomodoro phase. Unused by the
    /// stopwatch.
    total_ms: u64,
    /// The goal the user set for a plain timer. Kept while another mode is selected.
    timer_ms: u64,
    /// Time banked before the current run segment. While running, the segment adds to it.
    banked_ms: u64,
    /// When the current run segment started. None unless running.
    since: Option<Instant>,
    laps: Vec<Lap>,
    /// Focus sessions finished in the current cycle, 0 to `POMODORO_CYCLE`.
    focus_done: u8,
    /// When the alert card gives way. None when no card is up.
    alert_until: Option<Instant>,
    durations: Durations,
}

impl Default for Engine {
    fn default() -> Self {
        let mut engine = Self {
            mode: Mode::Timer,
            status: Status::Idle,
            phase: Phase::Idle,
            total_ms: DEFAULT_TIMER_MS,
            timer_ms: DEFAULT_TIMER_MS,
            banked_ms: 0,
            since: None,
            laps: Vec::new(),
            focus_done: 0,
            alert_until: None,
            durations: Durations::default(),
        };
        engine.reset_to_start();
        engine
    }
}

/// The error when a mode change would discard a run in progress.
pub const BUSY: &str = "A timer is running. Reset it before switching.";

impl Engine {
    /// Takes the pomodoro lengths from the config. A phase keeps the length it started with.
    pub fn set_durations(&mut self, durations: Durations) {
        self.durations = durations;
    }

    /// Whether a run is in progress, paused, or finished and waiting to be dismissed. This is what
    /// the compact pill shows.
    pub fn activity(&self) -> bool {
        matches!(self.status, Status::Running | Status::Paused | Status::Done)
    }

    /// Whether the alert card is up.
    pub fn alert(&self) -> bool {
        self.alert_until.is_some()
    }

    /// Switches mode, or sets the timer's goal, and returns the mode to its start. Refused while a
    /// run is in progress, so a tab click never discards a timer by accident.
    pub fn select(&mut self, mode: Mode, minutes: Option<u32>) -> Result<(), &'static str> {
        if matches!(self.status, Status::Running | Status::Paused) {
            return Err(BUSY);
        }
        self.mode = mode;
        if let Some(minutes) = minutes {
            self.timer_ms = minutes_to_ms(minutes.clamp(1, MAX_TIMER_MINUTES));
        }
        self.reset_to_start();
        Ok(())
    }

    /// Starts an idle clock. A paused one resumes through `resume`, and a finished one waits for a
    /// dismiss.
    pub fn start(&mut self, now: Instant) -> bool {
        if self.status != Status::Idle {
            return false;
        }
        self.banked_ms = 0;
        self.run(now);
        true
    }

    pub fn resume(&mut self, now: Instant) -> bool {
        if self.status != Status::Paused {
            return false;
        }
        self.run(now);
        true
    }

    pub fn pause(&mut self, now: Instant) -> bool {
        if self.status != Status::Running {
            return false;
        }
        self.banked_ms = self.elapsed(now);
        self.since = None;
        self.status = Status::Paused;
        true
    }

    /// Returns the current mode to its start. The goal and the pomodoro lengths are kept.
    pub fn reset(&mut self) {
        self.reset_to_start();
    }

    /// Adds time to a countdown. A finished timer starts again with the added time left. Ignored by
    /// the stopwatch. Dismisses the alert card, since the user has answered it.
    pub fn add_ms(&mut self, ms: u64, now: Instant) -> bool {
        if self.mode == Mode::Stopwatch {
            return false;
        }
        self.total_ms = self.total_ms.saturating_add(ms).min(MAX_COUNTDOWN_MS);
        if self.mode == Mode::Timer {
            self.timer_ms = self.total_ms;
        }
        if self.status == Status::Done && self.total_ms > self.banked_ms {
            self.status = Status::Idle;
            self.run(now);
        }
        self.alert_until = None;
        true
    }

    /// Records a stopwatch lap while it runs. Refused past `MAX_LAPS`.
    pub fn lap(&mut self, now: Instant) -> bool {
        if self.mode != Mode::Stopwatch
            || self.status != Status::Running
            || self.laps.len() >= MAX_LAPS
        {
            return false;
        }
        let total = self.elapsed(now);
        let previous = self.laps.last().map_or(0, |lap| lap.total_ms);
        self.laps.push(Lap {
            total_ms: total,
            split_ms: total - previous,
        });
        true
    }

    /// Clears the alert card. A finished timer also goes back to idle, which ends the activity.
    pub fn dismiss(&mut self) -> bool {
        let had_alert = self.alert_until.take().is_some();
        let finished_timer = self.mode == Mode::Timer && self.status == Status::Done;
        if finished_timer {
            self.reset_to_start();
        }
        had_alert || finished_timer
    }

    /// Brings the clock up to `now`. Every countdown that is due ends, and a pomodoro starts its
    /// next phase at the moment the previous one ended, so late ticks do not drift the schedule.
    /// An alert whose hold has run out is cleared.
    pub fn tick(&mut self, now: Instant) -> Tick {
        let mut tick = Tick::default();
        if self.alert_until.is_some_and(|until| now >= until) {
            self.alert_until = None;
            tick.changed = true;
        }

        while self.status == Status::Running
            && self.mode != Mode::Stopwatch
            && self.elapsed(now) >= self.total_ms
        {
            tick.changed = true;
            let ended = self.phase;
            let ended_total = self.total_ms;
            let end = self.segment_end().unwrap_or(now);
            self.alert_until = Some(now + ALERT_HOLD);

            match self.mode {
                Mode::Timer => {
                    self.status = Status::Done;
                    self.banked_ms = self.total_ms;
                    self.since = None;
                    tick.finished.push(Finished {
                        mode: Mode::Timer,
                        phase: Phase::Idle,
                        next: None,
                        total_ms: ended_total,
                    });
                }
                Mode::Pomodoro => {
                    let next = self.advance_cycle(ended);
                    self.phase = next;
                    self.total_ms = self.durations.for_phase(next);
                    self.banked_ms = 0;
                    self.since = Some(end);
                    tick.finished.push(Finished {
                        mode: Mode::Pomodoro,
                        phase: ended,
                        next: Some(next),
                        total_ms: ended_total,
                    });
                }
                Mode::Stopwatch => break,
            }
        }
        tick
    }

    /// The next moment `tick` has work to do: the end of a running countdown, or the end of the
    /// alert card. None when neither is pending.
    pub fn next_deadline(&self) -> Option<Instant> {
        [self.segment_end(), self.alert_until]
            .into_iter()
            .flatten()
            .min()
    }

    /// The state as the frontend shows it at `now`. `wall_ms` is the unix time of `now`, and gives
    /// the frontend the countdown's end in its own clock.
    pub fn snapshot(&self, now: Instant, wall_ms: u64) -> Snapshot {
        let countdown = self.mode != Mode::Stopwatch;
        let elapsed = self.elapsed(now);
        let remaining = if countdown {
            self.total_ms.saturating_sub(elapsed)
        } else {
            0
        };
        let running = self.status == Status::Running;
        Snapshot {
            mode: self.mode,
            phase: if self.mode == Mode::Pomodoro {
                self.phase
            } else {
                Phase::Idle
            },
            status: self.status,
            running,
            total_ms: if countdown { self.total_ms } else { 0 },
            elapsed_ms: elapsed,
            remaining_ms: remaining,
            ends_at_ms: (running && countdown).then_some(wall_ms.saturating_add(remaining)),
            laps: self.laps.clone(),
            pomodoro: PomodoroView {
                session: self.session(),
                of: POMODORO_CYCLE,
                completed: self.focus_done,
            },
        }
    }

    fn run(&mut self, now: Instant) {
        self.since = Some(now);
        self.status = Status::Running;
    }

    fn elapsed(&self, now: Instant) -> u64 {
        let running = self
            .since
            .map_or(0, |since| millis(now.saturating_duration_since(since)));
        self.banked_ms.saturating_add(running)
    }

    /// The instant the current countdown segment reaches zero, while it runs.
    fn segment_end(&self) -> Option<Instant> {
        match (self.status, self.since) {
            (Status::Running, Some(since)) if self.mode != Mode::Stopwatch => Some(
                since + Duration::from_millis(self.total_ms.saturating_sub(self.banked_ms)),
            ),
            _ => None,
        }
    }

    fn session(&self) -> u8 {
        match self.phase {
            Phase::Focus => (self.focus_done + 1).min(POMODORO_CYCLE),
            _ => self.focus_done.clamp(1, POMODORO_CYCLE),
        }
    }

    /// The phase that follows `ended`, and the cycle count that goes with it.
    fn advance_cycle(&mut self, ended: Phase) -> Phase {
        match ended {
            Phase::Focus => {
                self.focus_done = (self.focus_done + 1).min(POMODORO_CYCLE);
                if self.focus_done >= POMODORO_CYCLE {
                    Phase::Long
                } else {
                    Phase::Break
                }
            }
            Phase::Long => {
                self.focus_done = 0;
                Phase::Focus
            }
            Phase::Break | Phase::Idle => Phase::Focus,
        }
    }

    /// Puts the current mode back at its start. Its goal and pomodoro lengths stay.
    fn reset_to_start(&mut self) {
        self.status = Status::Idle;
        self.banked_ms = 0;
        self.since = None;
        self.laps.clear();
        self.focus_done = 0;
        self.alert_until = None;
        match self.mode {
            Mode::Timer => {
                self.phase = Phase::Idle;
                self.total_ms = self.timer_ms;
            }
            Mode::Stopwatch => {
                self.phase = Phase::Idle;
                self.total_ms = 0;
            }
            Mode::Pomodoro => {
                self.phase = Phase::Focus;
                self.total_ms = self.durations.focus_ms;
            }
        }
    }
}

fn minutes_to_ms(minutes: u32) -> u64 {
    u64::from(minutes) * 60_000
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn wall_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, millis)
}

/// The engine behind a lock, plus the condition variable that wakes the worker. Managed as an
/// `Arc`, so the worker and the commands share it.
#[derive(Default)]
pub struct TimerHandle {
    engine: Mutex<Engine>,
    wake: Condvar,
}

impl TimerHandle {
    fn lock(&self) -> MutexGuard<'_, Engine> {
        // A poisoned lock still holds a valid engine, since every change is whole.
        self.engine.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub struct TimerModule;

impl Module for TimerModule {
    fn name(&self) -> &'static str {
        "timer"
    }

    fn start(&self, ctx: ModuleContext) {
        let handle = ctx.app.state::<Arc<TimerHandle>>().inner().clone();
        thread::spawn(move || run(ctx.app, ctx.island, handle));
    }
}

/// Sleeps until the engine's next deadline, or until a command wakes it, then brings the clock up
/// to date. Each wait holds the lock until it sleeps, so a command cannot slip between the check
/// and the wait.
fn run(app: AppHandle, island: SharedIsland, handle: Arc<TimerHandle>) {
    let mut engine = handle.lock();
    loop {
        let now = Instant::now();
        engine.set_durations(durations(&app));
        let tick = engine.tick(now);
        if tick.changed || !tick.finished.is_empty() {
            announce(&app, &island, &engine, now, &tick.finished);
        }

        let deadline = engine.next_deadline();
        engine = match deadline {
            Some(at) => {
                handle
                    .wake
                    .wait_timeout(engine, at.saturating_duration_since(now))
                    .unwrap_or_else(PoisonError::into_inner)
                    .0
            }
            None => handle.wake.wait(engine).unwrap_or_else(PoisonError::into_inner),
        };
    }
}

/// Sends the snapshot, the finishes and the activity to the island. The sound plays once for a
/// batch of finishes.
fn announce(
    app: &AppHandle,
    island: &SharedIsland,
    engine: &Engine,
    now: Instant,
    finished: &[Finished],
) {
    for done in finished {
        let _ = app.emit(FINISHED_EVENT, done);
    }
    if !finished.is_empty() && sound_enabled(app) {
        beep();
    }
    let _ = app.emit(UPDATE_EVENT, engine.snapshot(now, wall_ms()));
    let activity = engine.activity().then_some(ActivityKind::Timer);
    let alert = engine.alert().then_some(ActivityKind::Timer);
    state::commit(app, island, |machine| {
        machine.set_activity(activity);
        machine.set_alert(alert);
    });
}

/// The system's default beep. Failing to play is not worth reporting.
fn beep() {
    let _ = unsafe { MessageBeep(MB_OK) };
}

fn config_snapshot(app: &AppHandle) -> Option<Config> {
    app.try_state::<SharedConfig>()
        .map(|shared| config::lock(&shared).clone())
}

fn durations(app: &AppHandle) -> Durations {
    config_snapshot(app).map_or_else(Durations::default, |config| {
        Durations::from_minutes(
            config.focus_minutes,
            config.short_break_minutes,
            config.long_break_minutes,
        )
    })
}

fn sound_enabled(app: &AppHandle) -> bool {
    config_snapshot(app).is_none_or(|config| config.timer_sound)
}

/// Runs one change under the lock, brings the clock up to date, publishes the result, and wakes
/// the worker. Returns what the change said, and the snapshot to send back to the caller.
fn update<T>(app: &AppHandle, change: impl FnOnce(&mut Engine, Instant) -> T) -> (T, Snapshot) {
    let handle = app.state::<Arc<TimerHandle>>().inner().clone();
    let island = app.state::<SharedIsland>().inner().clone();
    let now = Instant::now();
    let (result, snapshot) = {
        let mut engine = handle.lock();
        engine.set_durations(durations(app));
        let result = change(&mut engine, now);
        let tick = engine.tick(now);
        announce(app, &island, &engine, now, &tick.finished);
        (result, engine.snapshot(now, wall_ms()))
    };
    handle.wake.notify_all();
    (result, snapshot)
}

#[tauri::command]
pub fn timer_snapshot(app: AppHandle) -> Snapshot {
    update(&app, |_, _| ()).1
}

/// Switches mode, or sets the timer's goal in minutes. Refused while a run is in progress.
#[tauri::command]
pub fn timer_select(app: AppHandle, mode: Mode, minutes: Option<u32>) -> Result<Snapshot, String> {
    let (result, snapshot) = update(&app, |engine, _| engine.select(mode, minutes));
    result.map(|()| snapshot).map_err(String::from)
}

#[tauri::command]
pub fn timer_start(app: AppHandle) -> Snapshot {
    update(&app, |engine, now| engine.start(now)).1
}

#[tauri::command]
pub fn timer_pause(app: AppHandle) -> Snapshot {
    update(&app, |engine, now| engine.pause(now)).1
}

#[tauri::command]
pub fn timer_resume(app: AppHandle) -> Snapshot {
    update(&app, |engine, now| engine.resume(now)).1
}

#[tauri::command]
pub fn timer_reset(app: AppHandle) -> Snapshot {
    update(&app, |engine, _| engine.reset()).1
}

#[tauri::command]
pub fn timer_add_ms(app: AppHandle, ms: u64) -> Snapshot {
    update(&app, |engine, now| engine.add_ms(ms, now)).1
}

#[tauri::command]
pub fn timer_lap(app: AppHandle) -> Snapshot {
    update(&app, |engine, now| engine.lap(now)).1
}

#[tauri::command]
pub fn timer_dismiss(app: AppHandle) -> Snapshot {
    update(&app, |engine, _| engine.dismiss()).1
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60_000;

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

    fn timer(minutes: u32) -> Engine {
        let mut engine = Engine::default();
        engine.select(Mode::Timer, Some(minutes)).unwrap();
        engine
    }

    fn pomodoro() -> Engine {
        let mut engine = Engine::default();
        engine.select(Mode::Pomodoro, None).unwrap();
        engine
    }

    fn stopwatch() -> Engine {
        let mut engine = Engine::default();
        engine.select(Mode::Stopwatch, None).unwrap();
        engine
    }

    #[test]
    fn a_fresh_timer_is_idle_at_five_minutes() {
        let engine = Engine::default();
        let snap = engine.snapshot(Instant::now(), 0);
        assert_eq!(snap.status, Status::Idle);
        assert_eq!(snap.total_ms, 5 * MIN);
        assert_eq!(snap.remaining_ms, 5 * MIN);
        assert_eq!(snap.ends_at_ms, None);
        assert!(!engine.activity());
    }

    #[test]
    fn pause_and_resume_keep_the_time_already_run() {
        let clock = Clock::new();
        let mut engine = timer(1);
        assert!(engine.start(clock.at(0)));
        assert!(engine.pause(clock.at(20_000)));

        let paused = engine.snapshot(clock.at(50_000), 1_000_000);
        assert_eq!(paused.status, Status::Paused);
        assert_eq!(paused.elapsed_ms, 20_000);
        assert_eq!(paused.remaining_ms, 40_000);
        assert_eq!(paused.ends_at_ms, None, "a paused countdown has no end time");

        assert!(engine.resume(clock.at(50_000)));
        let running = engine.snapshot(clock.at(60_000), 1_000_000);
        assert_eq!(running.elapsed_ms, 30_000);
        assert_eq!(running.remaining_ms, 30_000);
        assert_eq!(running.ends_at_ms, Some(1_000_000 + 30_000));
    }

    #[test]
    fn start_only_begins_an_idle_clock_and_resume_only_a_paused_one() {
        let clock = Clock::new();
        let mut engine = timer(1);
        assert!(!engine.resume(clock.at(0)));
        assert!(!engine.pause(clock.at(0)));
        assert!(engine.start(clock.at(0)));
        assert!(!engine.start(clock.at(1)));
        assert!(!engine.resume(clock.at(1)));
    }

    #[test]
    fn adding_time_extends_a_running_countdown() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));
        assert!(engine.add_ms(MIN, clock.at(10_000)));

        let snap = engine.snapshot(clock.at(10_000), 0);
        assert_eq!(snap.total_ms, 2 * MIN);
        assert_eq!(snap.remaining_ms, 110_000);
        assert_eq!(engine.snapshot(clock.at(10_000), 0).status, Status::Running);
    }

    #[test]
    fn reset_returns_to_the_goal_the_user_set() {
        let clock = Clock::new();
        let mut engine = timer(10);
        engine.start(clock.at(0));
        engine.pause(clock.at(5_000));
        engine.reset();

        let snap = engine.snapshot(clock.at(6_000), 0);
        assert_eq!(snap.status, Status::Idle);
        assert_eq!(snap.elapsed_ms, 0);
        assert_eq!(snap.total_ms, 10 * MIN);
    }

    #[test]
    fn a_countdown_finishes_once_at_zero_and_holds_an_alert() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));

        let early = engine.tick(clock.at(MIN - 1));
        assert!(early.finished.is_empty());
        assert!(!early.changed);

        let done = engine.tick(clock.at(MIN));
        assert_eq!(
            done.finished,
            vec![Finished {
                mode: Mode::Timer,
                phase: Phase::Idle,
                next: None,
                total_ms: MIN,
            }]
        );
        assert!(done.changed);
        assert!(engine.alert());
        assert!(engine.activity(), "a finished timer stays in the pill until dismissed");
        assert_eq!(engine.snapshot(clock.at(MIN), 0).status, Status::Done);
        assert_eq!(engine.snapshot(clock.at(MIN), 0).remaining_ms, 0);

        // The alert gives way after its hold. The finished timer stays.
        assert_eq!(engine.next_deadline(), Some(clock.at(MIN + 4_000)));
        let later = engine.tick(clock.at(MIN + 4_000));
        assert!(later.changed && later.finished.is_empty());
        assert!(!engine.alert());
        assert!(engine.activity());
    }

    #[test]
    fn a_finished_timer_is_not_finished_twice() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));
        engine.tick(clock.at(MIN));
        assert!(engine.tick(clock.at(MIN + 10_000)).finished.is_empty());
    }

    #[test]
    fn plus_one_minute_restarts_a_finished_timer_with_that_minute_left() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));
        engine.tick(clock.at(MIN));

        engine.add_ms(MIN, clock.at(MIN + 2_000));
        let snap = engine.snapshot(clock.at(MIN + 2_000), 0);
        assert_eq!(snap.status, Status::Running);
        assert_eq!(snap.total_ms, 2 * MIN);
        assert_eq!(snap.remaining_ms, MIN);
        assert!(!engine.alert(), "answering the card clears it");
    }

    #[test]
    fn dismiss_ends_a_finished_timer_and_its_activity() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));
        engine.tick(clock.at(MIN));

        assert!(engine.dismiss());
        assert!(!engine.activity());
        assert!(!engine.alert());
        assert_eq!(engine.snapshot(clock.at(MIN), 0).status, Status::Idle);
        assert!(!engine.dismiss(), "nothing left to dismiss");
    }

    #[test]
    fn a_mode_cannot_change_under_a_running_timer() {
        let clock = Clock::new();
        let mut engine = timer(1);
        engine.start(clock.at(0));
        assert_eq!(engine.select(Mode::Stopwatch, None), Err(BUSY));
        engine.pause(clock.at(1_000));
        assert_eq!(engine.select(Mode::Pomodoro, None), Err(BUSY));

        engine.reset();
        assert!(engine.select(Mode::Stopwatch, None).is_ok());
    }

    #[test]
    fn switching_mode_keeps_the_timer_goal() {
        let mut engine = timer(7);
        engine.select(Mode::Stopwatch, None).unwrap();
        engine.select(Mode::Timer, None).unwrap();
        assert_eq!(engine.snapshot(Instant::now(), 0).total_ms, 7 * MIN);
    }

    #[test]
    fn goals_are_clamped_to_the_longest_countdown() {
        let mut engine = Engine::default();
        engine.select(Mode::Timer, Some(100_000)).unwrap();
        assert_eq!(
            engine.snapshot(Instant::now(), 0).total_ms,
            u64::from(MAX_TIMER_MINUTES) * MIN
        );
        let clock = Clock::new();
        engine.start(clock.at(0));
        engine.add_ms(10 * MIN, clock.at(1));
        assert_eq!(
            engine.snapshot(clock.at(1), 0).total_ms,
            u64::from(MAX_TIMER_MINUTES) * MIN
        );
    }

    #[test]
    fn stopwatch_keeps_at_most_twenty_laps_with_their_splits() {
        let clock = Clock::new();
        let mut engine = stopwatch();
        assert!(!engine.lap(clock.at(0)), "no laps before the stopwatch runs");
        engine.start(clock.at(0));

        for n in 1..=25u64 {
            let accepted = engine.lap(clock.at(n * 1_000));
            assert_eq!(accepted, n <= 20, "lap {n}");
        }
        let snap = engine.snapshot(clock.at(25_000), 0);
        assert_eq!(snap.laps.len(), 20);
        assert_eq!(snap.laps[0], Lap { total_ms: 1_000, split_ms: 1_000 });
        assert_eq!(snap.laps[19], Lap { total_ms: 20_000, split_ms: 1_000 });
        assert_eq!(snap.remaining_ms, 0);
        assert_eq!(snap.total_ms, 0);
    }

    #[test]
    fn stopwatch_laps_measure_across_a_pause() {
        let clock = Clock::new();
        let mut engine = stopwatch();
        engine.start(clock.at(0));
        engine.pause(clock.at(4_000));
        engine.resume(clock.at(10_000));
        engine.lap(clock.at(12_000));
        assert_eq!(engine.snapshot(clock.at(12_000), 0).laps[0].total_ms, 6_000);
    }

    #[test]
    fn stopwatch_never_finishes_and_ignores_added_time() {
        let clock = Clock::new();
        let mut engine = stopwatch();
        engine.start(clock.at(0));
        assert!(engine.tick(clock.at(3_600_000)).finished.is_empty());
        assert!(!engine.add_ms(MIN, clock.at(3_600_000)));
        assert!(engine.activity());
        assert_eq!(engine.next_deadline(), None);
    }

    /// Focus 25, break 5, long break 15, four focus sessions per cycle: the cycle runs end to end.
    #[test]
    fn pomodoro_runs_four_focus_sessions_then_a_long_break() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));

        let mut at = 0;
        let expected = [
            (Phase::Focus, Phase::Break, 25),
            (Phase::Break, Phase::Focus, 5),
            (Phase::Focus, Phase::Break, 25),
            (Phase::Break, Phase::Focus, 5),
            (Phase::Focus, Phase::Break, 25),
            (Phase::Break, Phase::Focus, 5),
            (Phase::Focus, Phase::Long, 25),
            (Phase::Long, Phase::Focus, 15),
        ];
        for (ended, next, minutes) in expected {
            at += minutes * MIN;
            let tick = engine.tick(clock.at(at));
            assert_eq!(
                tick.finished,
                vec![Finished {
                    mode: Mode::Pomodoro,
                    phase: ended,
                    next: Some(next),
                    total_ms: minutes * MIN,
                }],
                "ending {ended:?} at {at} ms"
            );
            let snap = engine.snapshot(clock.at(at), 0);
            assert_eq!(snap.phase, next);
            assert_eq!(snap.status, Status::Running, "phases auto-advance");
        }

        // After the long break the cycle starts again at focus session one, with no dots filled.
        let snap = engine.snapshot(clock.at(at), 0);
        assert_eq!(snap.pomodoro, PomodoroView { session: 1, of: 4, completed: 0 });
        assert_eq!(snap.total_ms, 25 * MIN);
    }

    #[test]
    fn pomodoro_views_track_the_session_and_the_dots() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));
        assert_eq!(
            engine.snapshot(clock.at(0), 0).pomodoro,
            PomodoroView { session: 1, of: 4, completed: 0 }
        );

        engine.tick(clock.at(25 * MIN));
        assert_eq!(
            engine.snapshot(clock.at(25 * MIN), 0).pomodoro,
            PomodoroView { session: 1, of: 4, completed: 1 },
            "a short break after focus one still names session one"
        );

        engine.tick(clock.at(30 * MIN));
        assert_eq!(
            engine.snapshot(clock.at(30 * MIN), 0).pomodoro,
            PomodoroView { session: 2, of: 4, completed: 1 }
        );
    }

    /// A late tick must not push the next phase back: the break starts at the moment the focus
    /// ended, not at the moment the code noticed.
    #[test]
    fn a_late_tick_starts_the_next_phase_at_the_scheduled_time() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));

        let late = engine.tick(clock.at(26 * MIN));
        assert_eq!(late.finished.len(), 1);
        // The break began at 25 min, so it ends at 30 min, and the focus after it has 24 min left
        // one minute into it.
        let focus = engine.tick(clock.at(31 * MIN));
        assert_eq!(focus.finished.len(), 1, "the break ended at 30 min");
        assert_eq!(engine.snapshot(clock.at(31 * MIN), 0).remaining_ms, 24 * MIN);
    }

    #[test]
    fn several_phases_missed_in_one_gap_are_all_reported() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));
        // Focus, break and the next focus all end before this tick.
        let tick = engine.tick(clock.at(56 * MIN));
        assert_eq!(tick.finished.len(), 3);
        assert_eq!(engine.snapshot(clock.at(56 * MIN), 0).phase, Phase::Break);
    }

    #[test]
    fn pomodoro_reset_returns_to_focus_session_one() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));
        engine.tick(clock.at(25 * MIN));
        engine.reset();

        let snap = engine.snapshot(clock.at(26 * MIN), 0);
        assert_eq!(snap.status, Status::Idle);
        assert_eq!(snap.phase, Phase::Focus);
        assert_eq!(snap.pomodoro, PomodoroView { session: 1, of: 4, completed: 0 });
    }

    #[test]
    fn pomodoro_uses_the_configured_lengths() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.set_durations(Durations::from_minutes(10, 2, 3));
        engine.reset();
        assert_eq!(engine.snapshot(clock.at(0), 0).total_ms, 10 * MIN);
        engine.start(clock.at(0));
        engine.tick(clock.at(10 * MIN));
        assert_eq!(engine.snapshot(clock.at(10 * MIN), 0).total_ms, 2 * MIN);
    }

    #[test]
    fn a_running_phase_keeps_the_length_it_started_with() {
        let clock = Clock::new();
        let mut engine = pomodoro();
        engine.start(clock.at(0));
        engine.set_durations(Durations::from_minutes(1, 1, 1));
        assert_eq!(engine.snapshot(clock.at(0), 0).total_ms, 25 * MIN);
    }

    #[test]
    fn the_snapshot_serializes_for_the_frontend() {
        let clock = Clock::new();
        let mut engine = timer(2);
        engine.start(clock.at(0));
        let json = serde_json::to_value(engine.snapshot(clock.at(500), 1_700_000_000_000)).unwrap();
        assert_eq!(json["mode"], "timer");
        assert_eq!(json["phase"], "idle");
        assert_eq!(json["status"], "running");
        assert_eq!(json["running"], true);
        assert_eq!(json["totalMs"], 120_000);
        assert_eq!(json["elapsedMs"], 500);
        assert_eq!(json["remainingMs"], 119_500);
        assert_eq!(json["endsAtMs"], 1_700_000_119_500u64);
        assert_eq!(json["pomodoro"]["of"], 4);
        assert_eq!(json["laps"], serde_json::json!([]));

        let mut laps = stopwatch();
        laps.start(clock.at(0));
        laps.lap(clock.at(1_000));
        let json = serde_json::to_value(laps.snapshot(clock.at(1_000), 0)).unwrap();
        assert_eq!(json["mode"], "stopwatch");
        assert_eq!(json["endsAtMs"], serde_json::Value::Null);
        assert_eq!(json["laps"][0]["splitMs"], 1_000);
        assert_eq!(serde_json::to_value(Phase::Long).unwrap(), "long");
    }

    #[test]
    fn next_deadline_is_the_countdown_end_or_the_alert_end_whichever_is_first() {
        let clock = Clock::new();
        let mut engine = timer(1);
        assert_eq!(engine.next_deadline(), None);
        engine.start(clock.at(0));
        assert_eq!(engine.next_deadline(), Some(clock.at(MIN)));
        engine.tick(clock.at(MIN));
        assert_eq!(engine.next_deadline(), Some(clock.at(MIN + 4_000)));
    }

    /// A real-clock smoke test: the engine, read with the system's own `Instant`, finishes a
    /// countdown that is due, with no injected times.
    #[test]
    fn a_real_clock_countdown_finishes_when_its_time_is_up() {
        let mut engine = Engine {
            timer_ms: 30,
            total_ms: 30,
            ..Engine::default()
        };
        engine.start(Instant::now());
        thread::sleep(Duration::from_millis(60));
        let tick = engine.tick(Instant::now());
        assert_eq!(tick.finished.len(), 1);
        assert!(engine.alert());
    }

    #[test]
    fn durations_come_from_minutes_and_default_to_the_config_defaults() {
        assert_eq!(
            Durations::default(),
            Durations::from_minutes(25, 5, 15)
        );
        assert_eq!(Durations::from_minutes(1, 2, 3).long_ms, 3 * MIN);
    }
}
