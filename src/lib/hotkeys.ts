// The hotkey settings' view rules. Pure, so the action list, the accelerator text and the status line
// are tested without a window. The statuses mirror HotkeyStatus in src-tauri/src/modules/hotkeys.rs,
// which comes from hotkeys_status and hotkeys://status.

/** The four actions a global hotkey can run. Each is a field of HotkeySettings in config.ts. */
export type HotkeyAction = "playPause" | "nextTrack" | "previousTrack" | "timerToggle";

export const HOTKEY_ACTIONS: { id: HotkeyAction; label: string }[] = [
  { id: "playPause", label: "Play or pause" },
  { id: "nextTrack", label: "Next track" },
  { id: "previousTrack", label: "Previous track" },
  { id: "timerToggle", label: "Pause or resume the timer" },
];

/** Whether one binding took. `error` is the reason it did not, when `ok` is false. */
export interface HotkeyStatus {
  action: HotkeyAction;
  ok: boolean;
  error: string | null;
}

/** The accelerator as the person meant it: each key trimmed and the keys joined with "+". */
export function cleanAccelerator(text: string): string {
  return text
    .split("+")
    .map((key) => key.trim())
    .filter((key) => key !== "")
    .join("+");
}

/** The line to show under an action when its binding did not take, or null when it did or is not known. */
export function bindingError(statuses: HotkeyStatus[], action: HotkeyAction): string | null {
  const found = statuses.find((status) => status.action === action);
  if (!found || found.ok) return null;
  return found.error ?? "Another app already uses this shortcut.";
}
