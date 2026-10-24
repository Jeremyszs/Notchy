// The notch's motion system: three springs, each usable as a CSS easing (`linear()`), a JS easing for
// Svelte transitions and Web Animations, and a settle time. Pure, so the curves are tested in
// motion.test.ts. See DESIGN.md, "Motion".
//
// A spring is described as SwiftUI does: `response` is the period of the undamped oscillation in
// seconds, and `bounce` is 0 for no overshoot (critically damped) up to 1. The spring starts at rest at
// 0 and settles at 1.

import type { TransitionConfig } from "svelte/transition";

export type Spring = { response: number; bounce: number };

/** Presses, toggles and anything that shrinks. Fast, no overshoot. */
export const SNAPPY: Spring = { response: 0.28, bounce: 0 };
/** Content, pages and vertical motion. No overshoot, so a top edge never lifts off y=0. */
export const SMOOTH: Spring = { response: 0.42, bounce: 0 };
/** The notch growing wider. A small overshoot, peak about 1.03. Horizontal only. */
export const BOUNCY: Spring = { response: 0.48, bounce: 0.25 };

// The spring counts as settled once it stays this close to 1.
const SETTLE = 0.002;

/** Position of the spring at `t` seconds, from 0 towards 1. */
export function springAt(spring: Spring, t: number): number {
  const w0 = (2 * Math.PI) / spring.response;
  const zeta = 1 - Math.min(Math.max(spring.bounce, 0), 0.99);
  if (zeta >= 1) return 1 - Math.exp(-w0 * t) * (1 + w0 * t);
  const wd = w0 * Math.sqrt(1 - zeta * zeta);
  return 1 - Math.exp(-zeta * w0 * t) * (Math.cos(wd * t) + ((zeta * w0) / wd) * Math.sin(wd * t));
}

/** How long the spring takes to settle, in ms. The envelope bounds every later sample. */
export function settleMs(spring: Spring): number {
  const w0 = (2 * Math.PI) / spring.response;
  const zeta = 1 - Math.min(Math.max(spring.bounce, 0), 0.99);
  // Walk forward until the curve and every later swing stay inside the band.
  const step = 0.002;
  let t = 0;
  for (; t < 5; t += step) {
    const envelope = zeta >= 1 ? Math.exp(-w0 * t) * (1 + w0 * t) : Math.exp(-zeta * w0 * t) / Math.sqrt(1 - zeta * zeta);
    if (envelope < SETTLE) break;
  }
  return Math.round(t * 1000);
}

/** The spring as a function of progress 0..1 over its settle time, for Svelte and Web Animations. */
export function springEase(spring: Spring): (p: number) => number {
  const seconds = settleMs(spring) / 1000;
  return (p) => (p >= 1 ? 1 : p <= 0 ? 0 : springAt(spring, p * seconds));
}

/** The spring as a CSS `linear()` easing over its settle time. */
export function springCss(spring: Spring, samples = 48): string {
  const ease = springEase(spring);
  const points: string[] = [];
  for (let i = 0; i <= samples; i += 1) {
    points.push(String(Math.round(ease(i / samples) * 1000) / 1000));
  }
  return `linear(${points.join(", ")})`;
}

export const SNAPPY_MS = settleMs(SNAPPY);
export const SMOOTH_MS = settleMs(SMOOTH);
export const BOUNCY_MS = settleMs(BOUNCY);
export const snappyEase = springEase(SNAPPY);
export const smoothEase = springEase(SMOOTH);
export const bouncyEase = springEase(BOUNCY);

/**
 * The springs as custom properties on the root, so CSS reads the same curves:
 * `--spring-snappy`, `--spring-snappy-ms`, and the same for smooth and bouncy.
 */
export function motionProperties(): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [name, spring] of [["snappy", SNAPPY], ["smooth", SMOOTH], ["bouncy", BOUNCY]] as const) {
    out[`--spring-${name}`] = springCss(spring);
    out[`--spring-${name}-ms`] = `${settleMs(spring)}ms`;
  }
  return out;
}

/**
 * Content arriving: it rises a few px, sharpens from a light blur and fades in on the smooth spring.
 * Reduced motion keeps only the fade. Use as `in:rise` and pair with a short `out:fade`.
 */
export function rise(
  _node: Element,
  { y = 4, blur = 4, delay = 0, duration = SMOOTH_MS, reduced = false }: {
    y?: number;
    blur?: number;
    delay?: number;
    duration?: number;
    reduced?: boolean;
  } = {},
): TransitionConfig {
  if (reduced) return { delay, duration: 150, css: (t) => `opacity: ${t}` };
  return {
    delay,
    duration,
    easing: smoothEase,
    // Opacity leads the movement, so the text is readable before it has fully landed.
    css: (t, u) =>
      `opacity: ${Math.min(1, t * 1.6)}; transform: translateY(${u * y}px); filter: blur(${u * blur}px)`,
  };
}
