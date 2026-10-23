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
  idleClock: boolean;
  focusMinutes: number;
  shortBreakMinutes: number;
  longBreakMinutes: number;
  notifications: NotificationSettings;
  battery: BatterySettings;
  stats: StatsSettings;
  calendar: CalendarSettings;
}

// Mirrors Stats in config.rs.
export interface StatsSettings {
  enabled: boolean;
}

// Mirrors Calendar in config.rs. The events themselves are not here: they live in events.json.
export interface CalendarSettings {
  enabled: boolean;
  leadMinutes: number;
  allDayHour: number;
}

// Mirrors Battery in config.rs.
export interface BatterySettings {
  enabled: boolean;
  chargeAlerts: boolean;
  lowBatteryAlerts: boolean;
}

// Mirrors Notifications in config.rs. Apps seen so far are not here: they live in their own file.
export interface NotificationSettings {
  enabled: boolean;
  /** AppUserModelIds whose notifications are ignored. */
  deniedApps: string[];
}

// The backend defaults. The notch uses them until the first answer arrives.
export const DEFAULT_CONFIG: Config = {
  pages: ["media", "controls", "timer", "stats", "calendar"],
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
  idleClock: true,
  focusMinutes: 25,
  shortBreakMinutes: 5,
  longBreakMinutes: 15,
  notifications: { enabled: true, deniedApps: [] },
  battery: { enabled: true, chargeAlerts: true, lowBatteryAlerts: true },
  stats: { enabled: true },
  calendar: { enabled: true, leadMinutes: 10, allDayHour: 9 },
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
