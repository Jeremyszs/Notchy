// The timer's view rules. Pure: the store feeds in the backend snapshot and the clock, and gets the
// digits, ring and copy back, so the formats and the colours are tested without a window. The
// types mirror Snapshot and Finished in src-tauri/src/modules/timer.rs.

export type TimerMode = "timer" | "stopwatch" | "pomodoro";
export type TimerPhase = "idle" | "focus" | "break" | "long";
export type TimerStatus = "idle" | "running" | "paused" | "done";

export interface TimerLap {
  totalMs: number;
  splitMs: number;
}

export interface TimerSnapshot {
  mode: TimerMode;
  phase: TimerPhase;
  status: TimerStatus;
  running: boolean;
  /** The countdown length. Zero for the stopwatch. */
  totalMs: number;
  elapsedMs: number;
  remainingMs: number;
  /** Unix ms when a running countdown ends. Null otherwise. */
  endsAtMs: number | null;
  laps: TimerLap[];
  pomodoro: { session: number; of: number; completed: number };
}

export interface TimerFinished {
  mode: TimerMode;
  phase: TimerPhase;
  next: TimerPhase | null;
  totalMs: number;
}

/** The state before the backend answers. Matches the backend's default. */
export const IDLE_SNAPSHOT: TimerSnapshot = {
  mode: "timer",
  phase: "idle",
  status: "idle",
  running: false,
  totalMs: 5 * 60_000,
  elapsedMs: 0,
  remainingMs: 5 * 60_000,
  endsAtMs: null,
  laps: [],
  pomodoro: { session: 1, of: 4, completed: 0 },
};

/** The quick presets, in minutes. */
export const TIMER_PRESETS_MIN = [1, 5, 10, 25] as const;
export const MIN_TIMER_MINUTES = 1;
export const MAX_TIMER_MINUTES = 180;
/** The stopwatch keeps this many laps. Mirrors MAX_LAPS in the backend. */
export const MAX_LAPS = 20;
/** The countdown shows its last seconds with a small beat. */
export const BEAT_SECONDS = 10;

export interface Palette {
  from: string;
  to: string;
}

/**
 * The accent for a mode and phase: a gradient for the ring, the glow and the text. Timer is
 * Apple orange, focus is tomato, breaks are mint and the long break teal-blue, and the stopwatch
 * is white with a soft grey.
 */
export function palette(mode: TimerMode, phase: TimerPhase = "idle"): Palette {
  if (mode === "stopwatch") return { from: "#FFFFFF", to: "#8E8E93" };
  if (mode === "timer") return { from: "#FF9F0A", to: "#FFD60A" };
  if (phase === "break") return { from: "#30D158", to: "#7CF2A1" };
  if (phase === "long") return { from: "#64D2FF", to: "#64D2FF" };
  return { from: "#FF453A", to: "#FF7A59" };
}

/** Clamps a number to a range. A value that is not finite counts as the lower bound. */
export function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, value));
}

function pad2(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

function clockText(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${pad2(minutes)}:${pad2(seconds)}`
    : `${minutes}:${pad2(seconds)}`;
}

/**
 * A countdown's digits. Rounds up to whole seconds, so it reads 0:01 until it reaches zero, the
 * way a countdown does.
 */
export function formatClock(ms: number): string {
  return clockText(Math.max(0, Math.ceil(ms / 1000)));
}

/** A count-up in whole seconds, for the compact pill. */
export function formatElapsed(ms: number): string {
  return clockText(Math.max(0, Math.floor(ms / 1000)));
}

/** A stopwatch reading with hundredths, for the panel and the laps. */
export function formatStopwatch(ms: number): string {
  const centis = Math.floor(Math.max(0, ms) / 10);
  const seconds = Math.floor(centis / 100);
  return `${clockText(seconds)}.${pad2(centis % 100)}`;
}

/**
 * How far a countdown has left, as a fraction of its length. A finished countdown reads as
 * complete, so its ring stays full while the card is up.
 */
export function fractionLeft(totalMs: number, leftMs: number, done: boolean): number {
  if (done) return 1;
  if (totalMs <= 0) return 0;
  return clamp(leftMs / totalMs, 0, 1);
}

/**
 * The two rotations that draw a ring's remaining arc with transforms alone. Each half-disc turns
 * inside its half of the ring, so the visible arc runs clockwise from twelve o'clock. The right
 * half covers the first 180 degrees and the left half the rest. A rotation is 0 when that half is
 * full and -180 when it is empty.
 */
export function ringRotations(fraction: number): { right: number; left: number } {
  const angle = clamp(fraction, 0, 1) * 360;
  return {
    right: clamp(angle - 180, -180, 0),
    left: clamp(angle - 360, -180, 0),
  };
}

/**
 * The time until the countdown's displayed whole second changes, in ms. The display rounds up, so
 * it changes each time the remaining time passes a whole second. Zero left has no next change.
 */
export function msUntilNextSecond(leftMs: number): number {
  if (leftMs <= 0) return 0;
  return leftMs - 1000 * Math.floor((leftMs - 1) / 1000);
}

/** The view of the timer at one moment, for the island and the panel. */
export interface TimerView {
  mode: TimerMode;
  phase: TimerPhase;
  status: TimerStatus;
  running: boolean;
  /** Whether a run is in the compact pill: running, paused or waiting to be dismissed. */
  active: boolean;
  /** The digits in the pill. */
  label: string;
  elapsedMs: number;
  leftMs: number;
  totalMs: number;
  /** Share of the countdown left, for the ring. */
  fraction: number;
  /** Whether the countdown is running in its last seconds, when the digits beat. */
  lastTen: boolean;
  palette: Palette;
  pomodoro: TimerSnapshot["pomodoro"];
  laps: TimerLap[];
}

/**
 * The view at `nowMs`. `receivedMs` is when the snapshot arrived, in the same clock. A running
 * countdown counts down to its end time, and a running stopwatch counts up from its last value.
 * Between updates the view is extrapolated, and the backend is not asked.
 */
export function viewAt(snapshot: TimerSnapshot, nowMs: number, receivedMs: number): TimerView {
  const running = snapshot.running;
  const countdown = snapshot.mode !== "stopwatch";
  const elapsedMs = running
    ? snapshot.elapsedMs + Math.max(0, nowMs - receivedMs)
    : snapshot.elapsedMs;
  const leftMs = countdown
    ? running && snapshot.endsAtMs !== null
      ? Math.max(0, snapshot.endsAtMs - nowMs)
      : snapshot.remainingMs
    : 0;
  const done = snapshot.status === "done";
  const label = countdown ? formatClock(leftMs) : formatElapsed(elapsedMs);

  return {
    mode: snapshot.mode,
    phase: snapshot.phase,
    status: snapshot.status,
    running,
    active: snapshot.status !== "idle",
    label,
    elapsedMs,
    leftMs,
    totalMs: snapshot.totalMs,
    fraction: countdown ? fractionLeft(snapshot.totalMs, leftMs, done) : 0,
    lastTen: countdown && running && leftMs > 0 && leftMs <= BEAT_SECONDS * 1000,
    palette: palette(snapshot.mode, snapshot.phase),
    pomodoro: snapshot.pomodoro,
    laps: snapshot.laps,
  };
}

/**
 * How long until the view next changes on its own, in ms, or null when it is still. A countdown
 * changes at each whole second, and a stopwatch at each whole second for the pill.
 */
export function msUntilNextChange(view: TimerView): number | null {
  if (!view.running) return null;
  if (view.mode === "stopwatch") return 1000 - (view.elapsedMs % 1000);
  return msUntilNextSecond(view.leftMs);
}

const PHASE_NAME: Record<TimerPhase, string> = {
  idle: "Timer",
  focus: "Focus",
  break: "Short break",
  long: "Long break",
};

/**
 * The alert card's copy. The snapshot is the newest one, so for a pomodoro its length is the
 * length of the phase that has just started.
 */
export function alertCopy(
  finished: TimerFinished,
  snapshot: TimerSnapshot,
): { title: string; detail: string } {
  if (finished.mode !== "pomodoro") {
    return {
      title: "Time's up",
      detail: `Timer · ${formatClock(finished.totalMs)}`,
    };
  }
  const titles: Record<TimerPhase, string> = {
    idle: "Time's up",
    focus: "Focus complete",
    break: "Break over",
    long: "Long break over",
  };
  const next = finished.next ?? "focus";
  return {
    title: titles[finished.phase],
    detail: `${PHASE_NAME[next]} · ${formatClock(snapshot.totalMs)}`,
  };
}

/** The pomodoro line under the digits: which part of the cycle is running. */
export function phaseLabel(view: TimerView): string {
  if (view.phase === "focus") return `Focus ${view.pomodoro.session} of ${view.pomodoro.of}`;
  if (view.phase === "break") return "Short break";
  if (view.phase === "long") return "Long break";
  return "Focus";
}

/** The minutes in a countdown, rounded, for the stepper and the presets. */
export function minutesOf(totalMs: number): number {
  return Math.round(totalMs / 60_000);
}
