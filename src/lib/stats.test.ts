import { test } from "node:test";
import assert from "node:assert/strict";
import {
  HISTORY,
  NETWORK_FLOOR,
  diskDetail,
  gigabytes,
  memoryDetail,
  percentFigure,
  pushSample,
  rateFigure,
  rateText,
  sparkMax,
  sparkPath,
} from "./stats.ts";

const KIB = 1024;
const MIB = KIB * KIB;
const GIB = MIB * KIB;

test("a percentage is whole digits between 0 and 100, or no digits before a reading", () => {
  assert.deepEqual(percentFigure(62.4), { number: "62", unit: "%" });
  assert.deepEqual(percentFigure(62.6), { number: "63", unit: "%" });
  assert.deepEqual(percentFigure(100), { number: "100", unit: "%" });
  assert.deepEqual(percentFigure(140), { number: "100", unit: "%" });
  assert.deepEqual(percentFigure(-3), { number: "0", unit: "%" });
  assert.deepEqual(percentFigure(null), { number: null, unit: "%" });
  assert.deepEqual(percentFigure(Number.NaN), { number: null, unit: "%" });
});

test("a rate is KB/s below 1000 KB, then MB/s, then GB/s", () => {
  assert.deepEqual(rateFigure(0), { number: "0", unit: "KB/s" });
  assert.deepEqual(rateFigure(512 * KIB), { number: "512", unit: "KB/s" });
  assert.deepEqual(rateFigure(999.4 * KIB), { number: "999", unit: "KB/s" });
  // 999.6 KB/s would round to 1000, so it is shown in the next unit.
  assert.deepEqual(rateFigure(999.6 * KIB), { number: "1.0", unit: "MB/s" });
  assert.deepEqual(rateFigure(12.4 * MIB), { number: "12.4", unit: "MB/s" });
  assert.deepEqual(rateFigure(999.9 * MIB), { number: "999.9", unit: "MB/s" });
  assert.deepEqual(rateFigure(1.5 * GIB), { number: "1.5", unit: "GB/s" });
});

test("a rate that is negative reads as zero, and a missing one has no digits", () => {
  assert.deepEqual(rateFigure(-50), { number: "0", unit: "KB/s" });
  assert.deepEqual(rateFigure(null), { number: null, unit: "" });
  assert.deepEqual(rateFigure(Number.POSITIVE_INFINITY), { number: null, unit: "" });
  assert.equal(rateText(null), "–");
  assert.equal(rateText(2 * MIB), "2.0 MB/s");
});

test("the longest figures stay short: 999.9 MB/s and 100%", () => {
  assert.equal(rateText(999.9 * MIB), "999.9 MB/s");
  assert.equal(percentFigure(100).number?.length, 3);
});

test("memory and disk details are in gibibytes with one decimal", () => {
  assert.equal(gigabytes(16 * GIB), "16.0");
  assert.equal(gigabytes(7.4 * GIB), "7.4");
  assert.equal(gigabytes(-1), "0.0");
  assert.equal(
    memoryDetail({ used: 7.4 * GIB, total: 16 * GIB }),
    "7.4 of 16.0 GB",
  );
  assert.equal(diskDetail({ free: 212.5 * GIB }), "212.5 GB free");
});

test("history keeps the newest samples and skips missing ones", () => {
  assert.deepEqual(pushSample([], null), []);
  assert.deepEqual(pushSample([1], Number.NaN), [1]);
  assert.deepEqual(pushSample([1], 2), [1, 2]);

  let history: number[] = [];
  for (let i = 1; i <= HISTORY + 5; i++) history = pushSample(history, i);
  assert.equal(history.length, HISTORY);
  assert.equal(history[0], 6);
  assert.equal(history[HISTORY - 1], HISTORY + 5);
});

test("a sparkline scale is never below its floor, and follows a larger sample", () => {
  assert.equal(sparkMax([], NETWORK_FLOOR), NETWORK_FLOOR);
  assert.equal(sparkMax([100, 2000], NETWORK_FLOOR), NETWORK_FLOOR);
  assert.equal(sparkMax([100, NETWORK_FLOOR * 3], NETWORK_FLOOR), NETWORK_FLOOR * 3);
});

test("a sparkline needs two samples and a positive scale", () => {
  assert.equal(sparkPath([], 100, 10, 100), "");
  assert.equal(sparkPath([50], 100, 10, 100), "");
  assert.equal(sparkPath([50, 60], 100, 10, 0), "");
});

test("a full sparkline spans the box from the bottom to the top", () => {
  assert.equal(sparkPath([0, 100], 100, 10, 100, 2), "M0 10 L100 0");
});

test("a short sparkline grows in from the right and keeps its spacing", () => {
  // Two samples in a three-slot history sit in the right two thirds of the box.
  assert.equal(sparkPath([50, 100], 100, 10, 100, 3), "M50 5 L100 0");
});

test("sparkline values outside the scale are clamped into the box", () => {
  assert.equal(sparkPath([-20, 300], 100, 10, 100, 2), "M0 10 L100 0");
});
