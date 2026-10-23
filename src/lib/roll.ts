// Motion helpers for the timer's odometer digits. Pure, so the timing and the easing are tested
// without a window. Only transform and opacity are produced, so the browser can run them on the
// compositor.
import type { TransitionConfig } from "svelte/transition";

/** How long a changed digit takes to roll out and the new one to roll in. */
export const ROLL_MS = 220;
/** Reduced motion keeps a plain crossfade, and this is its length. */
export const CROSSFADE_MS = 150;

/**
 * A cubic Bezier easing, as CSS `cubic-bezier(x1, y1, x2, y2)` defines it. Solved with Newton's
 * method, with bisection as the fallback, so the curve matches the browser's own.
 */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): (x: number) => number {
  const cx = 3 * x1;
  const bx = 3 * (x2 - x1) - cx;
  const ax = 1 - cx - bx;
  const cy = 3 * y1;
  const by = 3 * (y2 - y1) - cy;
  const ay = 1 - cy - by;

  const sampleX = (t: number) => ((ax * t + bx) * t + cx) * t;
  const sampleY = (t: number) => ((ay * t + by) * t + cy) * t;
  const slopeX = (t: number) => (3 * ax * t + 2 * bx) * t + cx;

  const solveX = (x: number): number => {
    let t = x;
    for (let i = 0; i < 8; i++) {
      const error = sampleX(t) - x;
      if (Math.abs(error) < 1e-7) return t;
      const slope = slopeX(t);
      if (Math.abs(slope) < 1e-7) break;
      t -= error / slope;
    }
    let lo = 0;
    let hi = 1;
    t = x;
    while (lo < hi) {
      const at = sampleX(t);
      if (Math.abs(at - x) < 1e-7) return t;
      if (x > at) lo = t;
      else hi = t;
      t = (lo + hi) / 2;
    }
    return t;
  };

  return (x: number) => {
    if (x <= 0) return 0;
    if (x >= 1) return 1;
    return sampleY(solveX(x));
  };
}

/** The easing every roll uses. It starts quickly and settles, with no overshoot. */
export const ROLL_EASE = cubicBezier(0.2, 0.8, 0.2, 1);

/**
 * The vertical offset of a digit in percent of its own height. `dir` is 1 for a digit that comes
 * in from below and -1 for one that leaves upward. `progress` runs from 0 (the digit is in place)
 * to 1 (the digit is fully moved away).
 */
export function rollOffsetPercent(dir: 1 | -1, progress: number): number {
  return dir * progress * 100;
}

/** The style for one frame of a roll. `t` is the eased opacity, and `u` is 1 - t. */
export function rollFrame(t: number, u: number, dir: 1 | -1, reduce: boolean): string {
  if (reduce) return `opacity: ${t}`;
  return `opacity: ${t}; transform: translate3d(0, ${rollOffsetPercent(dir, u)}%, 0)`;
}

export interface RollParams {
  dir: 1 | -1;
  reduce: boolean;
}

/**
 * A Svelte transition for one digit. Use `in:roll` with dir 1 and `out:roll` with dir -1: the new
 * digit rises from below while the old one slides up and out.
 */
export function roll(_node: Element, { dir, reduce }: RollParams): TransitionConfig {
  return {
    duration: reduce ? CROSSFADE_MS : ROLL_MS,
    easing: ROLL_EASE,
    css: (t, u) => rollFrame(t, u, dir, reduce),
  };
}

/** Whether a character is a digit, which is the only kind that rolls. */
export function isDigit(ch: string): boolean {
  return ch >= "0" && ch <= "9" && ch.length === 1;
}
