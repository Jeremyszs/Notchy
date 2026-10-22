// Turns a stream of wheel events into page turns. Pure apart from the injected clock, so it can be
// checked without a DOM. Each call to feed returns -1 (previous page), 0 (nothing), or 1 (next).

export type Turn = -1 | 0 | 1;

export type WheelLike = Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode" | "timeStamp">;

// Tuning, in px and ms. A notch is one discrete event. A stream is many small ones.
const LINE_PX = 16;
const PAGE_PX = 100;
// A single event this large is a mouse notch, and turns the page on its own.
const NOTCH_PX = 50;
// Two notches closer than this are one burst. A fling tail arrives every ~16 ms.
const NOTCH_GAP_MS = 60;
// The page animation takes about this long. Turns closer than this are dropped.
const COOLDOWN_MS = 120;
// A stream silent for longer than this is a new gesture, and all state is cleared.
const QUIET_MS = 80;
// Precision streams turn the page once this much delta has built up in one direction.
const SUM_PX = 24;
// Opposite motion this large turns the page back.
const REVERSE_PX = 10;
// Opposite motion past this releases the lock, so the next forward motion counts as new.
const RELEASE_PX = 6;
// An event this much bigger than the one before it, after the stream was shrinking, is a new flick.
const GROWTH = 1.6;
const GROWTH_MIN_PX = 8;

/** Creates a classifier. `now` is only read when an event has no timeStamp. */
export function createWheelGesture(options: { now?: () => number } = {}) {
  const now = options.now ?? (() => performance.now());

  let lastT: number | null = null;
  let lastMag = 0;
  let shrinking = false;
  // The last turn, and its time. Turns wait out the cooldown.
  let lastTurnT = -Infinity;
  let lastTurnDir: Turn = 0;
  // While locked, the stream is the tail of the turn in lockDir, and trailing events do not turn.
  let lockDir: Turn = 0;
  // Signed delta built up in the current precision gesture, and unsigned reversal since then.
  let sum = 0;
  let reversed = 0;

  function reset(): void {
    lastMag = 0;
    shrinking = false;
    lastTurnT = -Infinity;
    lastTurnDir = 0;
    lockDir = 0;
    sum = 0;
    reversed = 0;
  }

  function turn(dir: Turn, t: number): Turn {
    lastTurnT = t;
    lastTurnDir = dir;
    lockDir = dir;
    sum = 0;
    reversed = 0;
    return dir;
  }

  return {
    feed(event: WheelLike): Turn {
      const t = event.timeStamp ?? now();
      const raw = Math.abs(event.deltaX) > Math.abs(event.deltaY) ? event.deltaX : event.deltaY;
      if (raw === 0) return 0;

      const scale = event.deltaMode === 1 ? LINE_PX : event.deltaMode === 2 ? PAGE_PX : 1;
      const d = raw * scale;
      const mag = Math.abs(d);
      const dir: Turn = d > 0 ? 1 : -1;

      const gap = lastT === null ? Infinity : t - lastT;
      if (gap > QUIET_MS) reset();

      const cooled = t - lastTurnT >= COOLDOWN_MS;
      const notch = event.deltaMode !== 0 || mag >= NOTCH_PX;
      const grew = shrinking && lastMag > 0 && mag > GROWTH * lastMag && mag > GROWTH_MIN_PX;

      let out: Turn = 0;
      if (lockDir === dir) {
        // Motion along the locked turn. Only a new notch or a new flick turns again.
        reversed = 0;
        sum = 0;
        if (cooled && (grew || (notch && gap >= NOTCH_GAP_MS))) out = turn(dir, t);
      } else if (lockDir !== 0) {
        // Motion against the locked turn. Enough of it turns back, and a little releases the lock.
        reversed += mag;
        if (reversed >= REVERSE_PX) out = turn(dir, t);
        else if (reversed > RELEASE_PX) lockDir = 0;
      } else if (lastTurnDir !== 0 && dir !== lastTurnDir) {
        // Released, and now going back the way the last turn came from.
        reversed += mag;
        if (reversed >= REVERSE_PX) out = turn(dir, t);
      } else {
        // Fresh gesture, or going on the way the last turn went.
        reversed = 0;
        if (notch) {
          if (cooled) out = turn(dir, t);
        } else {
          sum = Math.sign(sum) === dir ? sum + d : d;
          if (cooled && Math.abs(sum) >= SUM_PX) out = turn(dir, t);
        }
      }

      shrinking = lastMag > 0 && mag < lastMag;
      lastMag = mag;
      lastT = t;
      return out;
    },
  };
}
