import { writable, type Readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { FontId } from "../lib/font";

// Mirrors Config in src-tauri/src/config.rs. The id lists are in the order they are shown.
export interface Config {
  pages: string[];
  controls: string[];
  lyrics: boolean;
  confirmLock: boolean;
  launchAtLogin: boolean;
  font: FontId;
  hud: boolean;
  timerSound: boolean;
  focusMinutes: number;
  shortBreakMinutes: number;
  longBreakMinutes: number;
}

// The backend defaults. The notch uses them until the first answer arrives.
export const DEFAULT_CONFIG: Config = {
  pages: ["media", "controls", "timer"],
  controls: [
    "volume",
    "brightness",
    "wifi",
    "bluetooth",
    "airplane",
    "mute",
    "awake",
    "lock",
  ],
  lyrics: true,
  confirmLock: true,
  launchAtLogin: false,
  font: "inter",
  hud: true,
  timerSound: true,
  focusMinutes: 25,
  shortBreakMinutes: 5,
  longBreakMinutes: 15,
};

const store = writable<Config>(DEFAULT_CONFIG);

// Every save reaches every window as "config://update". The snapshot covers a save that was
// emitted before this listener attached. An event that arrives first is newer, so it wins.
let heard = false;
listen<Config>("config://update", (event) => {
  heard = true;
  store.set(event.payload);
});
invoke<Config>("config_get").then(
  (snapshot) => {
    if (!heard) store.set(snapshot);
  },
  () => {},
);

export const config: Readable<Config> = { subscribe: store.subscribe };
