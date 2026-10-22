import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mediaState, type MediaState } from "./media";
import {
  lineIndexAt,
  positionAt,
  type LyricsState,
  type LyricWindow,
} from "../lib/lyrics";

const NO_LYRICS: LyricsState = { key: "", status: "none", lines: [], plain: null };

// The Rust side pushes each change and keeps the latest one. The snapshot covers an update
// emitted before this listener attached. An event that arrives first is newer, so it wins.
export const lyricsState = readable<LyricsState>(NO_LYRICS, (set) => {
  let heard = false;
  const unlisten = listen<LyricsState>("lyrics://update", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<LyricsState>("lyrics_snapshot").then(
    (snapshot) => {
      if (!heard) set(snapshot);
    },
    () => {},
  );

  return () => {
    unlisten.then((stop) => stop());
  };
});

/**
 * The lyric lines around the playback position, or null when there are no synced lines.
 * Recomputed on every media or lyrics change. While playing, one timer is armed for the
 * start of the next line, so nothing polls. A seek arrives as a media update and recomputes.
 */
export const currentLine = readable<LyricWindow | null>(null, (set) => {
  let media: MediaState | undefined;
  let lyrics: LyricsState = NO_LYRICS;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last: LyricWindow | null = null;

  const publish = (next: LyricWindow | null) => {
    const same =
      last === next ||
      (last !== null &&
        next !== null &&
        last.prev === next.prev &&
        last.current === next.current &&
        last.next === next.next);
    if (same) return;
    last = next;
    set(next);
  };

  const refresh = () => {
    clearTimeout(timer);
    timer = undefined;

    const lines = lyrics.status === "synced" ? lyrics.lines : [];
    if (!media || lines.length === 0) {
      publish(null);
      return;
    }

    const position = positionAt(media, Date.now());
    const index = lineIndexAt(lines, position);
    publish({
      prev: lines[index - 1]?.text ?? "",
      current: index >= 0 ? lines[index].text : "",
      next: lines[index + 1]?.text ?? "",
    });

    const upcoming = lines[index + 1];
    if (media.playing && upcoming) {
      // The 5 ms floor means a wake that lands a little early re-arms for the remainder,
      // instead of spinning at zero delay.
      timer = setTimeout(refresh, Math.max(upcoming.timeMs - position, 0) + 5);
    }
  };

  const unsubscribeMedia = mediaState.subscribe((value) => {
    media = value;
    refresh();
  });
  const unsubscribeLyrics = lyricsState.subscribe((value) => {
    lyrics = value;
    refresh();
  });

  return () => {
    clearTimeout(timer);
    unsubscribeMedia();
    unsubscribeLyrics();
  };
});
