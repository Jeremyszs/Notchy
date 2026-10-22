import { readable } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { type HudUpdate } from "../lib/hud";

// What the HUD last showed. The Rust side emits it on every accepted change, and decides when
// the HUD state starts and ends (see islandState). The value is kept after the HUD ends, so the
// shape can shrink away with its last content.
export const DEFAULT_HUD: HudUpdate = { kind: "volume", level: 0, muted: false };

export const hud = readable<HudUpdate>(DEFAULT_HUD, (set) => {
  const unlisten = listen<HudUpdate>("hud://update", (event) => {
    set(event.payload);
  });

  return () => {
    unlisten.then((stop) => stop());
  };
});
