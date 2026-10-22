import { derived, writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { config } from "./config";

// Mirrors ControlInfo in the controls backend. Sliders run 0..100, toggles are 0 or 1,
// and buttons carry no value. `detail` says why a control is unsupported.
export type ControlKind = "toggle" | "slider" | "button";

export interface ControlInfo {
  id: string;
  label: string;
  kind: ControlKind;
  supported: boolean;
  value: number;
  detail?: string | null;
}

// Null until the first list arrives, so the page can tell "loading" from "unavailable".
export const controls = writable<ControlInfo[] | null>(null);

// The controls the panel shows, in order, from the settings.
export const enabledControls = derived(config, ($config) => $config.controls);

// Enabled controls that the backend reported, in the enabled order.
export const visibleControls = derived(
  [controls, enabledControls],
  ([$controls, $enabled]) => {
    if (!$controls) return [];
    return $enabled.flatMap((id) => {
      const control = $controls.find((c) => c.id === id);
      return control ? [control] : [];
    });
  },
);

// Slider drags send at most once per interval, always with the latest value.
const DRAG_INTERVAL_MS = 60;

// The last value the backend accepted per control, to revert to when a set fails.
const confirmed = new Map<string, number>();
// Every local change and every send is stamped on one clock. A reply only lands when no
// local change came after its send, so an old reply never overwrites a newer drag.
let clock = 0;
const changedAt = new Map<string, number>();
const lastSent = new Map<string, number>();
const drags = new Map<string, { timer?: ReturnType<typeof setTimeout>; value: number }>();

function patch(id: string, change: Partial<ControlInfo>): void {
  controls.update((list) =>
    list?.map((c) => (c.id === id ? { ...c, ...change } : c)) ?? null,
  );
}

function applyLocal(id: string, value: number): void {
  changedAt.set(id, ++clock);
  patch(id, { value });
}

function send(id: string, value: number): Promise<void> {
  const sentAt = clock;
  lastSent.set(id, Date.now());
  const superseded = () => (changedAt.get(id) ?? 0) > sentAt;

  return invoke<ControlInfo>("controls_set", { id, value }).then(
    (fresh) => {
      if (superseded()) return;
      confirmed.set(id, fresh.value);
      patch(id, fresh);
    },
    () => {
      const was = confirmed.get(id);
      if (superseded() || was === undefined) return;
      patch(id, { value: was });
    },
  );
}

/** Loads every control with its current value. Called when the panel opens, not on a timer. */
export function refresh(): Promise<void> {
  return invoke<ControlInfo[]>("controls_list").then(
    (list) => {
      for (const c of list) confirmed.set(c.id, c.value);
      controls.set(list);
    },
    // Keep what is on screen. Only an empty first load shows as unavailable.
    () => controls.update((list) => list ?? []),
  );
}

/** Sets a control at once. The local value changes first, and the reply then reconciles it. */
export function setValue(id: string, value: number): Promise<void> {
  cancelDrag(id);
  applyLocal(id, value);
  return send(id, value);
}

/** A slider moved mid-drag. Updates at once, and sends at most once per DRAG_INTERVAL_MS. */
export function dragValue(id: string, value: number): void {
  applyLocal(id, value);

  const drag = drags.get(id) ?? { value };
  drags.set(id, drag);
  drag.value = value;
  if (drag.timer !== undefined) return;

  const wait = Math.max(0, DRAG_INTERVAL_MS - (Date.now() - (lastSent.get(id) ?? 0)));
  drag.timer = setTimeout(() => {
    drag.timer = undefined;
    void send(id, drag.value);
  }, wait);
}

/** The slider was released. Sends the final value whatever the throttle holds. */
export function endDrag(id: string, value: number): Promise<void> {
  return setValue(id, value);
}

/** Presses a button control. Failures are ignored. The control has no state to show. */
export function press(id: string): Promise<void> {
  return invoke<void>("controls_press", { id }).catch(() => {});
}

function cancelDrag(id: string): void {
  const drag = drags.get(id);
  if (!drag) return;
  clearTimeout(drag.timer);
  drags.delete(id);
}

// Mirrors WifiNetwork in src-tauri/src/modules/controls/network.rs. `signal` is 0..100 and
// `known` means the network has a saved profile, so it can join without a password.
export interface WifiNetwork {
  ssid: string;
  signal: number;
  secured: boolean;
  connected: boolean;
  known: boolean;
}

// Mirrors BtDevice in the same file. `kind` is a coarse guess from the device class.
export type BtKind = "audio" | "input" | "phone" | "other";

export interface BtDevice {
  id: string;
  name: string;
  connected: boolean;
  kind: BtKind;
}

/** The error the backend sends when location access blocks Wi-Fi scans. */
export const LOCATION_HINT =
  "Allow location access for desktop apps to list networks";

/** Scans and lists the Wi-Fi networks in range. Called when the sub-view opens, never on a timer. */
export function wifiNetworks(): Promise<WifiNetwork[]> {
  return invoke<WifiNetwork[]>("wifi_networks");
}

/** Joins a saved network. The backend refuses any other, so a password is never asked for here. */
export function joinWifi(ssid: string): Promise<void> {
  return invoke<void>("wifi_connect", { ssid });
}

/** Lists the paired Bluetooth devices. Read-only: Windows offers no API to connect them. */
export function bluetoothDevices(): Promise<BtDevice[]> {
  return invoke<BtDevice[]>("bluetooth_devices");
}

/** Opens one Windows Settings page. The backend accepts only these three names. */
export function openSettings(
  page: "wifi" | "bluetooth" | "location",
): Promise<void> {
  return invoke<void>("open_settings", { page });
}
