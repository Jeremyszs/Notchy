import { test } from "node:test";
import assert from "node:assert/strict";
import { BOUNCY, SMOOTH, SNAPPY, motionProperties, settleMs, springAt, springCss, springEase } from "./motion.ts";

function peak(spring: typeof SMOOTH): number {
  let max = 0;
  for (let t = 0; t < 2; t += 0.001) max = Math.max(max, springAt(spring, t));
  return max;
}

test("every spring starts at 0 and its easing ends exactly at 1", () => {
  for (const spring of [SNAPPY, SMOOTH, BOUNCY]) {
    assert.equal(springAt(spring, 0), 0);
    const ease = springEase(spring);
    assert.equal(ease(0), 0);
    assert.equal(ease(1), 1);
    assert.ok(Math.abs(ease(0.999) - 1) < 0.01);
  }
});

test("the snappy and smooth springs never overshoot, so a top edge never lifts", () => {
  assert.ok(peak(SNAPPY) <= 1);
  assert.ok(peak(SMOOTH) <= 1);
});

test("the bouncy spring peaks about 3% past its rest point", () => {
  const top = peak(BOUNCY);
  assert.ok(top > 1.02 && top < 1.04, `peak ${top}`);
});

test("settle times grow with the response and stay in a usable range", () => {
  assert.ok(settleMs(SNAPPY) < settleMs(SMOOTH));
  for (const spring of [SNAPPY, SMOOTH, BOUNCY]) {
    const ms = settleMs(spring);
    assert.ok(ms > 200 && ms < 900, `settle ${ms}`);
  }
});

test("the CSS easing is a linear() list from 0 to 1", () => {
  const css = springCss(SMOOTH, 10);
  assert.match(css, /^linear\(0, [\d., ]+, 1\)$/);
  assert.equal(css.split(",").length, 11);
  const props = motionProperties();
  assert.deepEqual(Object.keys(props).sort(), [
    "--spring-bouncy",
    "--spring-bouncy-ms",
    "--spring-smooth",
    "--spring-smooth-ms",
    "--spring-snappy",
    "--spring-snappy-ms",
  ]);
});
