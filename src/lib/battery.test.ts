import { test } from "node:test";
import assert from "node:assert/strict";
import {
  batteryColor,
  batteryLabel,
  batteryPercent,
  glyphFill,
  hasBolt,
  percentText,
  statusCopy,
} from "./battery.ts";

test("the level is a whole percent between 0 and 100", () => {
  assert.equal(batteryPercent(82.4), 82);
  assert.equal(batteryPercent(82.6), 83);
  assert.equal(batteryPercent(-5), 0);
  assert.equal(batteryPercent(140), 100);
  assert.equal(batteryPercent(Number.NaN), 0);
  assert.equal(percentText(9), "9%");
  assert.equal(percentText(200), "100%");
});

test("the glyph fill follows the level and stays inside the body", () => {
  assert.equal(glyphFill(0), 0);
  assert.equal(glyphFill(50), 0.5);
  assert.equal(glyphFill(100), 1);
  assert.equal(glyphFill(120), 1);
  assert.equal(glyphFill(-3), 0);
});

test("each kind has its words", () => {
  assert.equal(batteryLabel("charging"), "Charging");
  assert.equal(batteryLabel("unplugged"), "On battery");
  assert.equal(batteryLabel("full"), "Fully charged");
  assert.equal(batteryLabel("lowAmber"), "Low battery");
  assert.equal(batteryLabel("lowRed"), "Low battery");
});

test("the bolt is on the charger only", () => {
  assert.equal(hasBolt("charging"), true);
  assert.equal(hasBolt("full"), true);
  assert.equal(hasBolt("unplugged"), false);
  assert.equal(hasBolt("lowAmber"), false);
  assert.equal(hasBolt("lowRed"), false);
});

test("the low levels use amber at 20% and red at 10%, and the charger is green", () => {
  assert.equal(batteryColor("lowAmber"), "#FF9F0A");
  assert.equal(batteryColor("lowRed"), "#FF453A");
  assert.equal(batteryColor("charging"), "#30D158");
  assert.equal(batteryColor("full"), "#30D158");
  assert.equal(batteryColor("unplugged"), "#FFFFFF");
});

test("the status line says what the battery is doing, or that there is none", () => {
  assert.equal(statusCopy({ percent: 82, plugged: true }), "Battery 82%, charging");
  assert.equal(statusCopy({ percent: 82, plugged: false }), "Battery 82%, on battery");
  assert.equal(statusCopy({ percent: 100, plugged: true }), "Battery 100%, fully charged");
  assert.equal(statusCopy(null), "No battery detected");
  assert.equal(statusCopy(undefined), "Checking the battery.");
});
