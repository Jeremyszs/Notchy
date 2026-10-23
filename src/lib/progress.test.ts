import { test } from "node:test";
import assert from "node:assert/strict";
import {
  canSeek,
  clampPosition,
  formatClock,
  progressRatio,
  seekStep,
  seekTarget,
  SEEK_STEP_MS,
} from "./progress.ts";

test("a track seeks only when its length is known", () => {
  assert.equal(canSeek(215_000), true);
  assert.equal(canSeek(0), false);
  assert.equal(canSeek(-1), false);
  assert.equal(canSeek(Number.NaN), false);
  assert.equal(canSeek(Number.POSITIVE_INFINITY), false);
});

test("a position stays between zero and the end of a known track", () => {
  assert.equal(clampPosition(-500, 60_000), 0);
  assert.equal(clampPosition(61_000, 60_000), 60_000);
  assert.equal(clampPosition(30_000, 60_000), 30_000);
  assert.equal(clampPosition(Number.NaN, 60_000), 0);
  // With no known length the position is only kept from going below zero.
  assert.equal(clampPosition(90_000, 0), 90_000);
  assert.equal(clampPosition(-1, 0), 0);
});

test("the fill is the share of the track played, and zero when the length is unknown", () => {
  assert.equal(progressRatio(0, 200_000), 0);
  assert.equal(progressRatio(50_000, 200_000), 0.25);
  assert.equal(progressRatio(200_000, 200_000), 1);
  assert.equal(progressRatio(250_000, 200_000), 1);
  assert.equal(progressRatio(50_000, 0), 0);
});

test("a point on the bar seeks to its share of the length, clamped to the bar", () => {
  assert.equal(seekTarget(0.5, 200_000), 100_000);
  assert.equal(seekTarget(0, 200_000), 0);
  assert.equal(seekTarget(1, 200_000), 200_000);
  assert.equal(seekTarget(-0.3, 200_000), 0);
  assert.equal(seekTarget(1.4, 200_000), 200_000);
  assert.equal(seekTarget(Number.NaN, 200_000), 0);
  assert.equal(seekTarget(0.5, 0), 0);
});

test("an arrow key moves the position by one step, and stops at the ends", () => {
  assert.equal(SEEK_STEP_MS, 5_000);
  assert.equal(seekStep(60_000, SEEK_STEP_MS, 200_000), 65_000);
  assert.equal(seekStep(60_000, -SEEK_STEP_MS, 200_000), 55_000);
  assert.equal(seekStep(2_000, -SEEK_STEP_MS, 200_000), 0);
  assert.equal(seekStep(198_000, SEEK_STEP_MS, 200_000), 200_000);
  assert.equal(seekStep(10_000, SEEK_STEP_MS, 0), 15_000);
});

test("the clock reads m:ss, with seconds always two digits", () => {
  assert.equal(formatClock(0), "0:00");
  assert.equal(formatClock(9_999), "0:09");
  assert.equal(formatClock(65_000), "1:05");
  assert.equal(formatClock(215_900), "3:35");
  assert.equal(formatClock(62 * 60_000 + 3_000), "62:03");
  assert.equal(formatClock(-4_000), "0:00");
  assert.equal(formatClock(Number.NaN), "0:00");
});
