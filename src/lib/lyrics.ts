// Pure helpers for synced lyrics. They read no stores, so they can be checked on their own.

// Mirrors LyricsUpdate in src-tauri/src/modules/lyrics.rs.
export type LyricsStatus = "loading" | "synced" | "plain" | "none";

export interface LyricLine {
  timeMs: number;
  text: string;
}

export interface LyricsState {
  key: string;
  status: LyricsStatus;
  lines: LyricLine[];
  plain: string | null;
}

// The lines around the playback position. Empty strings mark the ends of the list.
export interface LyricWindow {
  prev: string;
  current: string;
  next: string;
}

/** Index of the last line that has started by `positionMs`, or -1 before the first line. */
export function lineIndexAt(lines: readonly LyricLine[], positionMs: number): number {
  let low = 0;
  let high = lines.length - 1;
  let found = -1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (lines[mid].timeMs <= positionMs) {
      found = mid;
      low = mid + 1;
    } else {
      high = mid - 1;
    }
  }
  return found;
}

/** Playback position at `nowMs`. Extrapolates from the last sample only while playing. */
export function positionAt(
  media: { playing: boolean; positionMs: number; positionAtMs: number },
  nowMs: number,
): number {
  return media.positionMs + (media.playing ? Math.max(0, nowMs - media.positionAtMs) : 0);
}
