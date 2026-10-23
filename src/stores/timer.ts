import { readable, writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  IDLE_SNAPSHOT,
  msUntilNextChange,
  viewAt,
  type TimerFinished,
  type TimerMode,
  type TimerSnapshot,
  type TimerView,
} from "../lib/timer";

// The backend owns the clock (src-tauri/src/modules/timer.rs). Every change arrives on
// timer://update, with the time it was received so the view can extrapolate. timer://finished
// says which countdown just ended, and carries an id, so the same finish is never shown twice.

interface Received {
  snapshot: TimerSnapshot;
  receivedMs: number;
}

export interface Finish {
  id: number;
  data: TimerFinished;
}

const received = writable<Received>({
  snapshot: IDLE_SNAPSHOT,
  receivedMs: 0,
});

/** The latest snapshot, as the backend sent it. Read with `get` in actions, and by the view. */
export const timerState = { subscribe: received.subscribe };

/** The newest countdown that ended. Null until one does. */
export const lastFinish = writable<Finish | null>(null);

let finishSeq = 0;

function accept(snapshot: TimerSnapshot): void {
  received.set({ snapshot, receivedMs: Date.now() });
}

// Attached once, when the module loads, so an update that arrives early is not lost.
listen<TimerSnapshot>("timer://update", (event) => accept(event.payload));
listen<TimerFinished>("timer://finished", (event) => {
  finishSeq += 1;
  lastFinish.set({ id: finishSeq, data: event.payload });
});
invoke<TimerSnapshot>("timer_snapshot").then(
  (snapshot) => {
    // An update that arrived first is newer than this snapshot, so it is kept.
    if (get(received).receivedMs === 0) accept(snapshot);
  },
  () => {},
);

/**
 * The timer as it looks now. Re-derived on every update, and again at each whole second while
 * it runs, so the digits, ring and beat track the clock without a request to the backend.
 */
export const timerView = readable<TimerView>(viewAt(IDLE_SNAPSHOT, 0, 0), (set) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let current = get(received);

  const refresh = () => {
    clearTimeout(timer);
    timer = undefined;
    const now = Date.now();
    const view = viewAt(current.snapshot, now, current.receivedMs);
    set(view);
    const wait = msUntilNextChange(view);
    if (wait !== null) timer = setTimeout(refresh, Math.max(1, wait));
  };

  const unsubscribe = received.subscribe((next) => {
    current = next;
    refresh();
  });

  return () => {
    clearTimeout(timer);
    unsubscribe();
  };
});

function call(command: string, args?: Record<string, unknown>): Promise<void> {
  return invoke<TimerSnapshot>(command, args).then(accept, () => {});
}

/** Switches to a mode, or sets the timer's goal in minutes. Refused while a run is in progress. */
export function selectMode(mode: TimerMode, minutes?: number): Promise<string | null> {
  return invoke<TimerSnapshot>("timer_select", { mode, minutes: minutes ?? null }).then(
    (snapshot) => {
      accept(snapshot);
      return null;
    },
    (error: unknown) => String(error),
  );
}

export const startTimer = () => call("timer_start");
export const pauseTimer = () => call("timer_pause");
export const resumeTimer = () => call("timer_resume");
export const resetTimer = () => call("timer_reset");
export const lapTimer = () => call("timer_lap");
export const dismissTimer = () => call("timer_dismiss");

/** Adds time to the countdown. Used for "+1 min" on the alert card and in the panel. */
export const addTime = (ms: number) => call("timer_add_ms", { ms });
