import { test } from "node:test";
import assert from "node:assert/strict";
import { countRows, DEFAULT_FIT, fitTrack, type Measure } from "./lyricsFit.ts";

// A fake measure: 10 px per character at 27 px, and the same per character at any other size, so
// a line's width scales with its font size. At 27 px a 100-character line is 1000 px.
const measure: Measure = (text, px) => text.length * 10 * (px / 27);

// The rows a text takes at 27 px, in a column of `width` px before the 4 px safety margin.
const rows27 = (text: string, width: number) => countRows(text, 27, 600, width, measure);

test("an empty track gets the size at rest", () => {
  assert.deepEqual(fitTrack([], measure), { w: 620, h: 224, scale: 1 });
  assert.deepEqual(fitTrack([], measure), DEFAULT_FIT);
});

test("short lines stay at the size at rest", () => {
  assert.deepEqual(fitTrack(["Hello there", "a short line", "and one more"], measure), {
    w: 620,
    h: 224,
    scale: 1,
  });
});

test("a line of two rows already fits the rest size", () => {
  // 40 characters need two rows at 620, and the 64 px box holds two rows.
  assert.deepEqual(fitTrack(["short", "x".repeat(40), "short"], measure), { w: 620, h: 224, scale: 1 });
});

test("a long line widens the pill, and the width is the smallest that holds it", () => {
  // The 100-character line is a context line on its neighbours' rows, so the width is the first at
  // which it is two rows in the column. With the 4 px margin and whole characters per row, 691 px
  // holds 49 characters a row (98 in all) and 695 px holds 50, so the answer is 695.
  const fit = fitTrack(["short line", "y".repeat(100), "short line"], measure);
  assert.deepEqual(fit, { w: 695, h: 224, scale: 1 });
});

test("when the widest pill is not enough, the height grows to hold the extra rows", () => {
  // At 700 the 100-character line takes three rows, and its 60-character neighbour two. That needs
  // 190 px of body, so the pill is 236 px tall, inside the 240 px window.
  const fit = fitTrack(["x".repeat(60), "y".repeat(100), "short"], measure);
  assert.deepEqual(fit, { w: 700, h: 236, scale: 1 });
});

test("a line too long even for the tallest pill scales the text, one step at a time", () => {
  // 220 characters take six rows at 700 px, which needs 254 px. At 0.95 it is still six rows, and
  // at 0.9 the body needs 207 px, which is over 240 with the chrome. At 0.8 it is five rows, 232 px.
  const fit = fitTrack(["y".repeat(220)], measure);
  assert.deepEqual(fit, { w: 700, h: 232, scale: 0.8 });
});

test("an absurdly long line scales to the floor and stays inside the window", () => {
  const fit = fitTrack(["z ".repeat(2000)], measure);
  assert.deepEqual(fit, { w: 700, h: 240, scale: 0.7 });
});

test("the fit is deterministic", () => {
  const lines = ["short", "x".repeat(60), "y".repeat(100), "short"];
  assert.deepEqual(fitTrack(lines, measure), fitTrack(lines, measure));
});

test("the fit depends on every line of the track, so it holds while the lines change", () => {
  const plain = ["short line", "short line"];
  const withLong = [...plain, "y".repeat(100)];
  assert.deepEqual(fitTrack(plain, measure), { w: 620, h: 224, scale: 1 });
  assert.notDeepEqual(fitTrack(withLong, measure), fitTrack(plain, measure));
});

test("a line that exactly fills a row wraps, because the column is counted with a 4 px margin", () => {
  // 36 characters are 360 px. A 364 px column counts 360 px after the margin, so the row is full.
  assert.equal(rows27("x".repeat(36), 364), 1);
  // 37 characters are 370 px, more than the 360 px counted, so they break onto a second row.
  assert.equal(rows27("x".repeat(37), 364), 2);
  // The same 36 characters wrap in a 363 px column (359 px after the margin).
  assert.equal(rows27("x".repeat(36), 363), 2);
});

test("a word wider than a row breaks between characters, and each row takes what fits", () => {
  // A 398 px column counts 394 px: 39 characters a row. 118 characters take 39 + 39 + 39 + 1 = 4 rows.
  // Counting by width alone gives 3, and the browser shows 4.
  assert.equal(rows27("x".repeat(118), 398), 4);
});

test("a space is dropped where a row breaks, so only the words count", () => {
  // Two 40 px words and a 10 px space: 90 px. A 94 px column counts 90 px and holds them on one row.
  assert.equal(rows27("aaaa bbbb", 94), 1);
  // A 90 px column counts 86 px, so the second word starts a new row.
  assert.equal(rows27("aaaa bbbb", 90), 2);
});

test("CJK characters break one by one, with no space needed", () => {
  // 30 characters of 10 px in a 200 px column counts 196 px, so 19 a row: two rows.
  assert.equal(rows27("中".repeat(30), 200), 2);
  // A Latin word and CJK characters after it break at the characters, without a space.
  assert.equal(rows27("ab" + "中".repeat(19), 200), 2);
});

test("every size stays within the rest size, the largest pill and the scale floor", () => {
  const tracks = [["short"], ["x".repeat(40)], ["y".repeat(300)], ["z ".repeat(900)]];
  for (const lines of tracks) {
    const fit = fitTrack(lines, measure);
    assert.ok(fit.w >= 620 && fit.w <= 700, `width ${fit.w}`);
    assert.ok(fit.h >= 224 && fit.h <= 240, `height ${fit.h}`);
    assert.ok(fit.scale >= 0.7 && fit.scale <= 1, `scale ${fit.scale}`);
  }
});
