import { test } from "node:test";
import assert from "node:assert/strict";
import { bindingError, cleanAccelerator, HOTKEY_ACTIONS, type HotkeyStatus } from "./hotkeys.ts";

test("there are four actions, each with its own id and label", () => {
  const ids = HOTKEY_ACTIONS.map((action) => action.id);
  assert.deepEqual(ids, ["playPause", "nextTrack", "previousTrack", "timerToggle"]);
  assert.equal(new Set(HOTKEY_ACTIONS.map((action) => action.label)).size, 4);
});

test("an accelerator is tidied to keys joined with plus signs", () => {
  assert.equal(cleanAccelerator("Ctrl+Alt+Space"), "Ctrl+Alt+Space");
  assert.equal(cleanAccelerator("  ctrl + alt +  t "), "ctrl+alt+t");
  assert.equal(cleanAccelerator("   "), "");
  assert.equal(cleanAccelerator(""), "");
});

test("a failed binding shows its reason, and a working one shows nothing", () => {
  const statuses: HotkeyStatus[] = [
    { action: "playPause", ok: true, error: null },
    { action: "nextTrack", ok: false, error: "Already registered by another app." },
    { action: "timerToggle", ok: false, error: null },
  ];
  assert.equal(bindingError(statuses, "playPause"), null);
  assert.equal(bindingError(statuses, "nextTrack"), "Already registered by another app.");
  assert.equal(bindingError(statuses, "timerToggle"), "Another app already uses this shortcut.");
  assert.equal(bindingError([], "previousTrack"), null);
});
