import { derived, readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { islandState } from "./island";
import { mediaState } from "./media";

// Mirrors the audio://levels event from src-tauri/src/modules/audio.rs. The payload is one
// level per bar, 0 to 1, at about 30 Hz while analysis is running.
interface LevelsEvent {
  levels: number[];
}

export const BAR_COUNT = 4;
// A bar never drops to nothing, so a silent track still shows four stubs.
const MIN_SCALE = 0.12;
// Start and stop wait this long for the request to settle, so a quick change of state does
// not start and stop the capture.
const DEBOUNCE_MS = 150;
// Live levels stop after this long without a frame. The bars then fall back to the CSS animation.
const LIVE_TIMEOUT_MS = 400;

const reduceMotion =
  typeof matchMedia === "function" &&
  matchMedia("(prefers-reduced-motion: reduce)").matches;

// Analysis runs only while the compact pill is the one on screen and media is playing. Every
// other island state, and a paused track, turns it off.
const wanted = derived(
  [islandState, mediaState],
  ([$state, $media]) => $state === "active" && $media.playing && !reduceMotion,
);

const clamp = (value: number) =>
  Number.isFinite(value) ? Math.min(1, Math.max(MIN_SCALE, value)) : MIN_SCALE;

/**
 * The bar scales, one per bar, or null when no live frame is current. Null keeps the CSS
 * fallback animation running. The store is set once per frame, and the island writes the four
 * transforms from this one array.
 */
export const levels = readable<number[] | null>(null, (set) => {
  let want = false;
  let sent = false;
  let flip: ReturnType<typeof setTimeout> | undefined;
  let stale: ReturnType<typeof setTimeout> | undefined;

  // The backend call is fire-and-forget. A failure (command missing, capture refused) is
  // swallowed, and the bars simply stay on the fallback.
  const sync = () => {
    flip = undefined;
    if (want === sent) return;
    sent = want;
    if (!want) {
      clearTimeout(stale);
      set(null);
    }
    void invoke("audio_set_active", { active: want }).catch(() => {});
  };

  const unsubscribe = wanted.subscribe((next) => {
    if (next === want) return;
    want = next;
    clearTimeout(flip);
    flip = setTimeout(sync, DEBOUNCE_MS);
  });

  const unlisten = listen<LevelsEvent>("audio://levels", (event) => {
    if (!sent || !want) return;
    const values = event.payload.levels ?? [];
    set(Array.from({ length: BAR_COUNT }, (_, i) => clamp(values[i] ?? 0)));
    clearTimeout(stale);
    stale = setTimeout(() => set(null), LIVE_TIMEOUT_MS);
  });

  return () => {
    unsubscribe();
    clearTimeout(flip);
    clearTimeout(stale);
    if (sent) void invoke("audio_set_active", { active: false }).catch(() => {});
    sent = false;
    unlisten.then((stop) => stop());
  };
});
