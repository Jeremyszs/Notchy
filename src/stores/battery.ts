import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_UPDATE,
  type BatteryReading,
  type BatteryUpdate,
} from "../lib/battery";

// The Rust side owns the pill and decides when the battery state starts and ends (see islandState).
// Each pill arrives on battery://show before the state changes. The last one is kept after it
// hides, so the pill can shrink away with its own content.
export const battery = readable<BatteryUpdate>(EMPTY_UPDATE, (set) => {
  const unlisten = listen<BatteryUpdate>("battery://show", (event) => {
    set(event.payload);
  });

  return () => {
    unlisten.then((stop) => stop());
  };
});

// The latest reading, for the settings window. undefined until it is read, and null on a PC with
// no battery. It is sent when it changes, and read once on subscribe.
export const batteryStatus = readable<BatteryReading | null | undefined>(undefined, (set) => {
  let heard = false;
  const unlisten = listen<BatteryReading | null>("battery://status", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<BatteryReading | null>("battery_snapshot").then(
    (snapshot) => {
      // An update that arrived first is newer than this snapshot, so it is kept.
      if (!heard) set(snapshot);
    },
    () => {},
  );

  return () => {
    unlisten.then((stop) => stop());
  };
});
