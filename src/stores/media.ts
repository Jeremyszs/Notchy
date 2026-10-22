import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Mirrors MediaUpdate in src-tauri/src/modules/media.rs. While playing, the position is
// extrapolated from positionMs as of positionAtMs (unix epoch ms). A durationMs of 0 is unknown.
export interface MediaState {
  active: boolean;
  playing: boolean;
  title: string;
  artist: string;
  album: string;
  source: string;
  art: string | null;
  durationMs: number;
  positionMs: number;
  positionAtMs: number;
}

const NOTHING_PLAYING: MediaState = {
  active: false,
  playing: false,
  title: "",
  artist: "",
  album: "",
  source: "",
  art: null,
  durationMs: 0,
  positionMs: 0,
  positionAtMs: 0,
};

// The Rust side pushes changes and keeps the latest one. The snapshot covers an update
// that was emitted before this listener attached.
export const mediaState = readable<MediaState>(NOTHING_PLAYING, (set) => {
  const unlisten = listen<MediaState>("media://update", (event) => {
    set(event.payload);
  });
  invoke<MediaState>("media_snapshot").then(set, () => {});

  return () => {
    unlisten.then((stop) => stop());
  };
});

// Fire-and-forget: the commands only enqueue, and the outcome shows up as an update.
export const playPause = () => invoke<void>("media_play_pause");
export const next = () => invoke<void>("media_next");
export const previous = () => invoke<void>("media_previous");
