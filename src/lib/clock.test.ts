import { test } from "node:test";
import assert from "node:assert/strict";
import { formatHM, msUntilNextMinute } from "./clock.ts";

test("the next minute is the time left to the next whole minute", () => {
  assert.equal(msUntilNextMinute(new Date(2026, 9, 23, 10, 30, 15, 250)), 44_750);
  assert.equal(msUntilNextMinute(new Date(2026, 9, 23, 10, 30, 59, 999)), 1);
});

test("exactly on a minute boundary the next change is a full minute away, never zero", () => {
  assert.equal(msUntilNextMinute(new Date(2026, 9, 23, 10, 31, 0, 0)), 60_000);
});

test("the idle clock reads HH:MM on a 24-hour clock with leading zeros", () => {
  assert.equal(formatHM(new Date(2026, 9, 23, 9, 5)), "09:05");
  assert.equal(formatHM(new Date(2026, 9, 23, 14, 32)), "14:32");
  assert.equal(formatHM(new Date(2026, 9, 23, 0, 0)), "00:00");
});
