import { test } from "node:test";
import assert from "node:assert/strict";
import {
  activityLabel,
  alertCopy,
  cycleDots,
  formatClock,
  formatElapsed,
  formatStopwatch,
  fractionLeft,
  IDLE_SNAPSHOT,
  msUntilNextChange,
  msUntilNextSecond,
  palette,
  phaseLabel,
  ringRotations,
  viewAt,
  type TimerSnapshot,
} from "./timer.ts";

const snap = (patch: Partial<TimerSnapshot>): TimerSnapshot => ({ ...IDLE_SNAPSHOT, ...patch });

test("countdown digits round up, so the last second reads 0:01 and zero reads 0:00", () => {
  assert.equal(formatClock(272_400), "4:33");
  assert.equal(formatClock(272_000), "4:32");
  assert.equal(formatClock(1), "0:01");
  assert.equal(formatClock(0), "0:00");
  assert.equal(formatClock(-50), "0:00");
  assert.equal(formatClock(3_725_000), "1:02:05");
});

test("the pill's count-up floors to whole seconds", () => {
  assert.equal(formatElapsed(999), "0:00");
  assert.equal(formatElapsed(61_999), "1:01");
});

test("stopwatch readings carry hundredths", () => {
  assert.equal(formatStopwatch(0), "0:00.00");
  assert.equal(formatStopwatch(1_234), "0:01.23");
  assert.equal(formatStopwatch(59_999), "0:59.99");
  assert.equal(formatStopwatch(3_600_000 + 5_060), "1:00:05.06");
});

test("the ring fraction is the share of the countdown left, and a finished ring is full", () => {
  assert.equal(fractionLeft(60_000, 30_000, false), 0.5);
  assert.equal(fractionLeft(60_000, 90_000, false), 1);
  assert.equal(fractionLeft(60_000, -1, false), 0);
  assert.equal(fractionLeft(0, 0, false), 0);
  assert.equal(fractionLeft(60_000, 0, true), 1);
});

test("ring rotations draw the remaining arc from twelve o'clock with two half discs", () => {
  assert.deepEqual(ringRotations(1), { right: 0, left: 0 }, "a full ring");
  assert.deepEqual(ringRotations(0), { right: -180, left: -180 }, "an empty ring");
  assert.deepEqual(ringRotations(0.25), { right: -90, left: -180 }, "a quarter, all in the right half");
  assert.deepEqual(ringRotations(0.5), { right: 0, left: -180 }, "half: right full, left empty");
  assert.deepEqual(ringRotations(0.75), { right: 0, left: -90 }, "three quarters");
  assert.deepEqual(ringRotations(2), { right: 0, left: 0 }, "clamped above one");
  assert.deepEqual(ringRotations(Number.NaN), { right: -180, left: -180 });
});

test("ring rotations never leave their range and only ever move one half at a time", () => {
  for (let i = 0; i <= 1000; i++) {
    const { right, left } = ringRotations(i / 1000);
    assert.ok(right >= -180 && right <= 0);
    assert.ok(left >= -180 && left <= 0);
    assert.ok(right === 0 || left === -180, "the left half only moves once the right is full");
  }
});

test("the next display change is the next whole second of the countdown", () => {
  assert.equal(msUntilNextSecond(3_500), 500);
  assert.equal(msUntilNextSecond(3_000), 1_000);
  assert.equal(msUntilNextSecond(1), 1);
  assert.equal(msUntilNextSecond(0), 0);
});

test("a countdown counts down to its end time while it runs", () => {
  const running = snap({
    status: "running",
    running: true,
    totalMs: 60_000,
    elapsedMs: 10_000,
    remainingMs: 50_000,
    endsAtMs: 1_000_000 + 50_000,
  });
  // Received at 1_000_000 and read at 1_020_000: the end is 30 s away, whatever the snapshot said.
  const view = viewAt(running, 1_020_000, 1_000_000);
  assert.equal(view.leftMs, 30_000);
  assert.equal(view.label, "0:30");
  assert.equal(view.fraction, 0.5);
  assert.equal(msUntilNextChange(view), msUntilNextSecond(30_000));
  assert.equal(msUntilNextSecond(30_000), 1_000);
});

test("a paused countdown holds its remaining time and has no next change", () => {
  const paused = snap({
    status: "paused",
    running: false,
    totalMs: 60_000,
    elapsedMs: 10_000,
    remainingMs: 50_000,
    endsAtMs: null,
  });
  const view = viewAt(paused, 9_999_999, 1_000_000);
  assert.equal(view.leftMs, 50_000);
  assert.equal(view.active, true, "paused stays in the pill");
  assert.equal(msUntilNextChange(view), null);
});

test("a stopwatch counts up from its last value while it runs", () => {
  const running = snap({
    mode: "stopwatch",
    status: "running",
    running: true,
    totalMs: 0,
    elapsedMs: 4_000,
    remainingMs: 0,
  });
  const view = viewAt(running, 1_500, 1_000);
  assert.equal(view.elapsedMs, 4_500);
  assert.equal(view.label, "0:04");
  assert.equal(view.fraction, 0);
  assert.equal(view.lastTen, false, "a stopwatch never beats");
  assert.equal(msUntilNextChange(view), 500);
});

test("the last ten seconds of a running countdown are flagged for the beat", () => {
  const at = (leftMs: number) =>
    viewAt(
      snap({
        status: "running",
        running: true,
        totalMs: 60_000,
        remainingMs: leftMs,
        endsAtMs: 5_000 + leftMs,
      }),
      5_000,
      5_000,
    );
  assert.equal(at(10_000).lastTen, true);
  assert.equal(at(10_001).lastTen, false);
  assert.equal(at(1).lastTen, true);
  assert.equal(at(0).lastTen, false, "nothing beats once the time is up");
});

test("a finished timer reads zero, keeps its ring full and stays in the pill", () => {
  const done = viewAt(
    snap({ status: "done", totalMs: 300_000, remainingMs: 0, elapsedMs: 300_000 }),
    0,
    0,
  );
  assert.equal(done.label, "0:00");
  assert.equal(done.fraction, 1);
  assert.equal(done.active, true);
  assert.equal(msUntilNextChange(done), null);
});

test("idle is not in the pill", () => {
  assert.equal(viewAt(IDLE_SNAPSHOT, 0, 0).active, false);
});

test("accents follow the mode and the pomodoro phase", () => {
  assert.deepEqual(palette("timer"), { from: "#FF9F0A", to: "#FFD60A" });
  assert.deepEqual(palette("pomodoro", "focus"), { from: "#FF453A", to: "#FF7A59" });
  assert.deepEqual(palette("pomodoro", "break"), { from: "#30D158", to: "#7CF2A1" });
  assert.deepEqual(palette("pomodoro", "long"), { from: "#64D2FF", to: "#64D2FF" });
  assert.deepEqual(palette("stopwatch"), { from: "#FFFFFF", to: "#8E8E93" });
});

test("the finished card names the timer and what the next phase is", () => {
  const timer = alertCopy(
    { mode: "timer", phase: "idle", next: null, totalMs: 300_000 },
    IDLE_SNAPSHOT,
  );
  assert.deepEqual(timer, { title: "Time's up", detail: "Timer · 5:00" });

  const next = snap({ mode: "pomodoro", phase: "break", totalMs: 300_000 });
  assert.deepEqual(
    alertCopy({ mode: "pomodoro", phase: "focus", next: "break", totalMs: 1_500_000 }, next),
    { title: "Focus complete", detail: "Short break · 5:00" },
  );
  const long = snap({ mode: "pomodoro", phase: "long", totalMs: 900_000 });
  assert.deepEqual(
    alertCopy({ mode: "pomodoro", phase: "focus", next: "long", totalMs: 1_500_000 }, long),
    { title: "Focus complete", detail: "Long break · 15:00" },
  );
});

test("the pomodoro line names the session in focus", () => {
  const focus = viewAt(
    snap({ mode: "pomodoro", phase: "focus", pomodoro: { session: 2, of: 4, completed: 1 } }),
    0,
    0,
  );
  assert.equal(phaseLabel(focus), "Focus 2 of 4");
  const shortBreak = viewAt(snap({ mode: "pomodoro", phase: "break" }), 0, 0);
  assert.equal(phaseLabel(shortBreak), "Short break");
});

test("the activity pill names the mode, the paused or finished state, or the pomodoro part", () => {
  assert.equal(activityLabel(viewAt(snap({ mode: "timer", status: "running", running: true }), 0, 0)), "Timer");
  assert.equal(activityLabel(viewAt(snap({ mode: "stopwatch", status: "running", running: true }), 0, 0)), "Stopwatch");
  assert.equal(activityLabel(viewAt(snap({ status: "paused" }), 0, 0)), "Paused");
  assert.equal(activityLabel(viewAt(snap({ status: "done" }), 0, 0)), "Done");
  assert.equal(activityLabel(viewAt(snap({ mode: "pomodoro", phase: "focus", status: "running", running: true }), 0, 0)), "Focus");
  assert.equal(activityLabel(viewAt(snap({ mode: "pomodoro", phase: "break", status: "running", running: true }), 0, 0)), "Break");
  assert.equal(activityLabel(viewAt(snap({ mode: "pomodoro", phase: "long", status: "running", running: true }), 0, 0)), "Long break");
});

test("the pomodoro dots show one dot per focus session, filled as they are completed", () => {
  const cycle = viewAt(snap({ mode: "pomodoro", pomodoro: { session: 3, of: 4, completed: 2 } }), 0, 0);
  assert.deepEqual(cycleDots(cycle), { total: 4, done: 2 });
  assert.deepEqual(cycleDots(viewAt(snap({ mode: "timer" }), 0, 0)), { total: 0, done: 0 });
  assert.deepEqual(cycleDots(viewAt(snap({ mode: "stopwatch" }), 0, 0)), { total: 0, done: 0 });
});
