// The media progress bar's rules. Pure, so the clock text, the bar's fill and the seek target are
// tested without a window. Positions and lengths are milliseconds, as the media store sends them. A
// length of 0 means the length is unknown (a live stream, or no session), and then nothing can seek.

/** How far the arrow keys move the position, in ms. */
export const SEEK_STEP_MS = 5_000;

/** Whether a track has a known length, so a bar and a seek make sense. */
export function canSeek(durationMs: number): boolean {
  return Number.isFinite(durationMs) && durationMs > 0;
}

/** A position kept inside the track: never below zero, and never past the end when the end is known. */
export function clampPosition(positionMs: number, durationMs: number): number {
  const at = Number.isFinite(positionMs) ? Math.max(0, positionMs) : 0;
  return canSeek(durationMs) ? Math.min(at, durationMs) : at;
}

/** How much of the bar is filled, from 0 to 1. Zero when the length is unknown. */
export function progressRatio(positionMs: number, durationMs: number): number {
  if (!canSeek(durationMs)) return 0;
  return clampPosition(positionMs, durationMs) / durationMs;
}

/** The position a point on the bar seeks to. The ratio is clamped to the bar first. */
export function seekTarget(ratio: number, durationMs: number): number {
  if (!canSeek(durationMs) || !Number.isFinite(ratio)) return 0;
  return Math.round(Math.min(1, Math.max(0, ratio)) * durationMs);
}

/** The position one arrow-key step away from `positionMs`, kept inside the track. */
export function seekStep(positionMs: number, deltaMs: number, durationMs: number): number {
  return clampPosition(clampPosition(positionMs, durationMs) + deltaMs, durationMs);
}

/** m:ss for a time in ms. Minutes are not capped, so 62 minutes and 3 seconds reads 62:03. */
export function formatClock(ms: number): string {
  const total = Math.floor(clampPosition(ms, 0) / 1000);
  const seconds = String(total % 60).padStart(2, "0");
  return `${Math.floor(total / 60)}:${seconds}`;
}
