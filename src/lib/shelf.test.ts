import { test } from "node:test";
import assert from "node:assert/strict";
import { relativeAge, fileName, extension, wheelRoute, cubicBezier, defaultMode, movedBeyond, DRAG_START_PX, fileFamily, FAMILY_COLORS } from "./shelf.ts";

test("relative ages use whole minutes, hours and days, with future times held at now", () => {
  for (const [ms, label] of [[0, "now"], [59_999, "now"], [120_000, "2m"], [3_600_000, "1h"], [259_200_000, "3d"]] as const) {
    assert.equal(relativeAge(0, ms), label);
  }
  assert.equal(relativeAge(10, 0), "now");
});
test("path names and extension badges handle UNC, folders, shortcuts and long names", () => {
  assert.equal(fileName("C:\\folder\\a file.lnk"), "a file.lnk");
  assert.equal(fileName("\\\\server\\share\\folder\\"), "folder");
  assert.equal(extension("archive.tar.gz"), "gz");
  assert.equal(extension(".hidden"), "");
  assert.equal(extension("folder.with.dot", true), "");
  assert.equal(extension("file."), "");
  assert.equal(fileName("C:\\" + "a".repeat(300)), "a".repeat(300));
});
test("strip wheels are always consumed; outside wheels belong to pages", () => {
  assert.deepEqual(wheelRoute(true, 1, 20), { consumed: true, delta: 20 });
  assert.deepEqual(wheelRoute(true, -30, 20), { consumed: true, delta: -30 });
  assert.deepEqual(wheelRoute(true, 0, 0), { consumed: true, delta: 0 });
  assert.deepEqual(wheelRoute(false, 0, 100), { consumed: false, delta: 0 });
});
test("cubic bezier easing matches the CSS curves at their ends and at the linear case", () => {
  const linear = cubicBezier(0, 0, 1, 1);
  for (const x of [0, 0.25, 0.5, 0.75, 1]) assert.ok(Math.abs(linear(x) - x) < 1e-4);
  const island = cubicBezier(0.2, 0.8, 0.2, 1);
  assert.ok(Math.abs(island(0)) < 1e-6);
  assert.ok(Math.abs(island(1) - 1) < 1e-6);
  // An ease-out curve is ahead of the linear line in the first half.
  assert.ok(island(0.5) > 0.5);
});
test("the default mode is Text only when clipboard is the one switch on", () => {
  assert.equal(defaultMode(true, false), "text");
  assert.equal(defaultMode(true, true), "file");
  assert.equal(defaultMode(false, true), "file");
  assert.equal(defaultMode(false, false), "file");
});
test("a drag starts only past six pixels, measured as a straight line", () => {
  assert.equal(movedBeyond(0, 0, 4, 4, DRAG_START_PX), false);
  assert.equal(movedBeyond(0, 0, 6, 0, DRAG_START_PX), false);
  assert.equal(movedBeyond(0, 0, 6, 1, DRAG_START_PX), true);
  assert.equal(movedBeyond(10, 10, 10, -10, DRAG_START_PX), true);
});

test("file families follow the extension, case blind, and anything else is other", () => {
  assert.equal(fileFamily("PNG"), "image");
  assert.equal(fileFamily("pdf"), "pdf");
  assert.equal(fileFamily("xlsx"), "sheet");
  assert.equal(fileFamily("svelte"), "code");
  assert.equal(fileFamily(""), "other");
  assert.equal(fileFamily(undefined), "other");
  assert.equal(fileFamily("qqq"), "other");
  assert.equal(Object.keys(FAMILY_COLORS).length, 10);
});
