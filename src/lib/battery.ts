// The battery pill's and settings' view rules. Pure, so the copy, the glyph scale and the status
// line are tested without a window. The types mirror Update and Reading in
// src-tauri/src/modules/battery.rs.

/** What happened. `lowAmber` is 20% on battery and `lowRed` is 10%. */
export type BatteryKind = "charging" | "unplugged" | "full" | "lowAmber" | "lowRed";

/** One pill. `percent` is the level at that moment. */
export interface BatteryUpdate {
  kind: BatteryKind;
  percent: number;
}

/** The power status when a battery is present. */
export interface BatteryReading {
  percent: number;
  plugged: boolean;
}

export const EMPTY_UPDATE: BatteryUpdate = { kind: "unplugged", percent: 0 };

const LABELS: Record<BatteryKind, string> = {
  charging: "Charging",
  unplugged: "On battery",
  full: "Fully charged",
  lowAmber: "Low battery",
  lowRed: "Low battery",
};

// One colour per kind, drawn solid on the glyph. Green is the charger, white is neutral, and the
// two low levels are the Apple orange and red from DESIGN.md.
const COLORS: Record<BatteryKind, string> = {
  charging: "#30D158",
  unplugged: "#FFFFFF",
  full: "#30D158",
  lowAmber: "#FF9F0A",
  lowRed: "#FF453A",
};

/** The level as a whole percent, clamped to 0 to 100. A NaN level reads as 0. */
export function batteryPercent(percent: number): number {
  if (!Number.isFinite(percent)) return 0;
  return Math.round(Math.min(100, Math.max(0, percent)));
}

/** The text beside the glyph: what happened. */
export function batteryLabel(kind: BatteryKind): string {
  return LABELS[kind];
}

/** The percentage text, such as "82%". */
export function percentText(percent: number): string {
  return `${batteryPercent(percent)}%`;
}

/** The glyph's fill as a fraction, for scaleX. */
export function glyphFill(percent: number): number {
  return batteryPercent(percent) / 100;
}

/** The glyph's colour for a kind. */
export function batteryColor(kind: BatteryKind): string {
  return COLORS[kind];
}

/** Whether the glyph carries the charging bolt. Charging and full charge are on the charger. */
export function hasBolt(kind: BatteryKind): boolean {
  return kind === "charging" || kind === "full";
}

/**
 * The line in Settings. `undefined` is before the first reading, and `null` is a PC with no
 * battery.
 */
export function statusCopy(reading: BatteryReading | null | undefined): string {
  if (reading === undefined) return "Checking the battery.";
  if (reading === null) return "No battery detected";
  const state = reading.plugged
    ? reading.percent >= 100
      ? "fully charged"
      : "charging"
    : "on battery";
  return `Battery ${batteryPercent(reading.percent)}%, ${state}`;
}
