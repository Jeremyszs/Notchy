import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type IslandState =
  | "idle"
  | "active"
  | "activity"
  | "expanded"
  | "lyrics"
  | "tucked"
  | "hud"
  | "alert"
  | "hidden";

// The Rust side owns the state machine and pushes every change, so the store
// only mirrors it. Starts at "idle" until the first event arrives.
export const islandState = readable<IslandState>("idle", (set) => {
  const unlisten = listen<IslandState>("island://state", (event) => {
    set(event.payload);
  });

  return () => {
    unlisten.then((stop) => stop());
  };
});

let wideSent: boolean | null = null;

/**
 * Tells the Rust machine whether the media page has synced lyrics to show. Hover still decides
 * whether the wide layout appears, so leaving the island never waits on this call. Repeats are
 * skipped, so a reactive caller can run on every change.
 */
export function setWide(wide: boolean): void {
  if (wide === wideSent) return;
  wideSent = wide;
  void invoke("island_set_wide", { wide });
}
