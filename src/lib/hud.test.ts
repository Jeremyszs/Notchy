import { test } from "node:test";
import assert from "node:assert/strict";
import { hudFill, hudIcon, hudLabel, hudPercent, type HudUpdate } from "./hud.ts";

const volume = (level: number, muted = false): HudUpdate => ({ kind: "volume", level, muted });
const brightness = (level: number): HudUpdate => ({ kind: "brightness", level, muted: false });

test("levels clamp to whole percents and a bad level reads as zero", () => {
  assert.equal(hudPercent(volume(42.4)), 42);
  assert.equal(hudPercent(volume(42.5)), 43);
  assert.equal(hudPercent(volume(180)), 100);
  assert.equal(hudPercent(volume(-5)), 0);
  assert.equal(hudPercent(volume(Number.NaN)), 0);
});

test("volume waves follow the level, and silence has none", () => {
  assert.equal(hudIcon(volume(0)), "silent");
  assert.equal(hudIcon(volume(1)), "low");
  assert.equal(hudIcon(volume(49)), "low");
  assert.equal(hudIcon(volume(50)), "high");
  assert.equal(hudIcon(volume(100)), "high");
});

test("muted volume gets the slash whatever the level", () => {
  assert.equal(hudIcon(volume(70, true)), "muted");
  assert.equal(hudIcon(volume(0, true)), "muted");
});

test("brightness is always the sun", () => {
  assert.equal(hudIcon(brightness(0)), "sun");
  assert.equal(hudIcon(brightness(80)), "sun");
});

test("the bar fills to the level, and muted volume reads as empty", () => {
  assert.equal(hudFill(volume(25)), 0.25);
  assert.equal(hudFill(volume(25, true)), 0);
  assert.equal(hudFill(brightness(100)), 1);
});

test("the label is the percent, and muted volume shows 0%", () => {
  assert.equal(hudLabel(volume(63)), "63%");
  assert.equal(hudLabel(volume(63, true)), "0%");
  assert.equal(hudLabel(brightness(7)), "7%");
});
