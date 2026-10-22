// The volume and brightness HUD's view rules. Pure, so the icon and bar mapping is tested
// without a window. The payload mirrors HudUpdate in src-tauri/src/modules/hud.rs.
export type HudKind = "volume" | "brightness";

export interface HudUpdate {
  kind: HudKind;
  /** 0 to 100. */
  level: number;
  /** Only ever true for volume. */
  muted: boolean;
}

export type HudIcon = "silent" | "low" | "high" | "muted" | "sun";

/** The level clamped to a whole percent. A missing or NaN level reads as 0. */
export function hudPercent(update: HudUpdate): number {
  if (!Number.isFinite(update.level)) return 0;
  return Math.round(Math.min(100, Math.max(0, update.level)));
}

/** Which glyph shows. Volume waves follow the level, muted volume gets the slash, brightness is a sun. */
export function hudIcon(update: HudUpdate): HudIcon {
  if (update.kind === "brightness") return "sun";
  if (update.muted) return "muted";
  const level = hudPercent(update);
  if (level === 0) return "silent";
  return level < 50 ? "low" : "high";
}

/** The bar's fill as a fraction for scaleX. Muted volume reads as empty. */
export function hudFill(update: HudUpdate): number {
  return update.muted ? 0 : hudPercent(update) / 100;
}

/** The value text on the right. Muted volume shows 0%, the level it is at until unmuted. */
export function hudLabel(update: HudUpdate): string {
  return `${update.muted ? 0 : hudPercent(update)}%`;
}
