import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { pushSample, type StatsSnapshot } from "../lib/stats";

// The Rust sampler (src-tauri/src/modules/stats.rs) reads only while the Stats page is on screen,
// and sends each reading on stats://update. The history of each figure is kept here, for the
// sparklines.

export interface StatsView {
  snapshot: StatsSnapshot | null;
  history: { cpu: number[]; down: number[]; memory: number[]; disk: number[] };
}

const EMPTY: StatsView = {
  snapshot: null,
  history: { cpu: [], down: [], memory: [], disk: [] },
};

const store = writable<StatsView>(EMPTY);

/** The latest reading and the recent history of each figure. */
export const stats = { subscribe: store.subscribe };

// Attached once, when the module loads. Nothing is sent while the sampler is off.
listen<StatsSnapshot>("stats://update", (event) => {
  const reading = event.payload;
  store.update((view) => ({
    snapshot: reading,
    history: {
      cpu: pushSample(view.history.cpu, reading.cpu),
      down: pushSample(view.history.down, reading.network.down),
      memory: pushSample(view.history.memory, reading.memory.percent),
      disk: pushSample(view.history.disk, reading.disk.percent),
    },
  }));
});

let sampling = false;

/**
 * Starts or stops the sampler. Starting clears the history, so an old graph never joins a new one.
 * A repeat of the same answer is not sent.
 */
export function watchStats(on: boolean): void {
  if (on === sampling) return;
  sampling = on;
  if (on) store.set(EMPTY);
  invoke("stats_set_active", { active: on }).catch(() => {});
}
