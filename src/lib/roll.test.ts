import { test } from "node:test";
import assert from "node:assert/strict";
import {
  CROSSFADE_MS,
  ROLL_EASE,
  ROLL_MS,
  cubicBezier,
  isDigit,
  rollFrame,
  rollOffsetPercent,
} from "./roll.ts";

test("the bezier matches a straight line for the linear curve", () => {
  const linear = cubicBezier(0, 0, 1, 1);
  for (const x of [0, 0.1, 0.25, 0.5, 0.9, 1]) {
    assert.ok(Math.abs(linear(x) - x) < 1e-5, `linear at ${x}`);
  }
});

test("the roll easing starts and ends exactly and never overshoots", () => {
  assert.equal(ROLL_EASE(0), 0);
  assert.equal(ROLL_EASE(1), 1);
  let previous = 0;
  for (let i = 1; i <= 200; i++) {
    const value = ROLL_EASE(i / 200);
    assert.ok(value >= previous - 1e-9, "monotonic");
    assert.ok(value <= 1 + 1e-9, "no overshoot");
    previous = value;
  }
  // A cubic-bezier(.2,.8,.2,1) ease-out is well past halfway by the middle of its run.
  assert.ok(ROLL_EASE(0.5) > 0.8);
});

test("a digit rolls over 220 ms, and reduced motion crossfades in 150 ms", () => {
  assert.equal(ROLL_MS, 220);
  assert.equal(CROSSFADE_MS, 150);
});

test("a digit comes in from below and leaves upward", () => {
  // Entering: progress 1 is the digit one full height below, and progress 0 is in place.
  assert.equal(rollOffsetPercent(1, 1), 100);
  assert.equal(rollOffsetPercent(1, 0), 0);
  // Leaving: the same progress carries the digit up and out.
  assert.equal(rollOffsetPercent(-1, 1), -100);
  assert.equal(rollOffsetPercent(-1, 0.5), -50);
});

test("a roll frame moves with transform and fades with opacity only", () => {
  const frame = rollFrame(0.5, 0.5, 1, false);
  assert.equal(frame, "opacity: 0.5; transform: translate3d(0, 50%, 0)");
  assert.doesNotMatch(rollFrame(0.5, 0.5, -1, false), /top|left|height|width/);
});

test("reduced motion drops the travel and keeps the fade", () => {
  assert.equal(rollFrame(0.3, 0.7, 1, true), "opacity: 0.3");
  assert.equal(rollFrame(0.3, 0.7, -1, true), "opacity: 0.3");
});

test("only the ten digits roll, and every other character holds still", () => {
  for (const ch of "0123456789") assert.equal(isDigit(ch), true, ch);
  for (const ch of [":", ".", " ", "a", "", "10"]) assert.equal(isDigit(ch), false, JSON.stringify(ch));
});
