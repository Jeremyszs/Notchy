import { readable } from "svelte/store";
import { mediaState } from "./media";
import { tintFromImage, tintFromText } from "../lib/tint";

// The lyrics gradient colour. `id` changes with every new colour, so the view keys a layer on
// it and crossfades the old layer out while the new one fades in.
export interface TintLayer {
  id: number;
  color: string;
}

let sequence = 0;

/**
 * The tint of the current track. It is computed once per track (title, artist and art), never on
 * a timer. A position-only media update leaves the key unchanged and does no work. A result that
 * arrives after the track changed is dropped.
 */
export const tint = readable<TintLayer>({ id: 0, color: tintFromText("") }, (set) => {
  let key: string | null = null;

  const unsubscribe = mediaState.subscribe((media) => {
    const next = `${media.title}\u0000${media.artist}\u0000${media.art ?? ""}`;
    if (next === key) return;
    key = next;

    void tintFromImage(media.art, `${media.title} ${media.artist}`).then((color) => {
      if (key !== next) return;
      set({ id: ++sequence, color });
    });
  });

  return () => unsubscribe();
});
