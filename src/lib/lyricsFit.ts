// The lyrics layout's size for a track. Pure, with no DOM: the caller passes a text measure, so the
// fit is tested with a fake one. The bounds mirror state.rs (LYRICS_MIN and LYRICS_MAX), which holds
// the hit area for the size this returns. The window is 720 x 240 with 10 px ears, so 700 x 240 is
// its largest body.

export type LyricsFit = { w: number; h: number; scale: number };

/** Measures a string's width in px at a font size and weight. */
export type Measure = (text: string, px: number, weight: number) => number;

const MIN_W = 620;
const MIN_H = 224;
const MAX_W = 700;
const MAX_H = 240;

// Mirrors the layout in Island.svelte (`.wide`, `.wide-body`, `.side`, `.stack`). Keep in sync.
// The stack column is the width less 16 px of side padding on each side, the 250 px left column
// and the 20 px gap between them.
const STACK_CHROME_W = 16 * 2 + 250 + 20;
// Top padding 14, the 8 px gap above the pager and the 24 px pager row: the body is h less 46.
const CHROME_H = 14 + 8 + 24;
// Two 8 px gaps between the previous, current and next lines.
const STACK_GAP = 16;
// Current line: 27 px, weight 600, 32 px line box, at least two rows (64 px box).
const CUR = { px: 27, weight: 600, line: 32, minRows: 2 };
// Context lines: 21 px, weight 400, 26 px line box, at least one row.
const CTX = { px: 21, weight: 400, line: 26, minRows: 1 };

/** The size at rest, and the size of an empty track. */
export const DEFAULT_FIT: Readonly<LyricsFit> = Object.freeze({ w: MIN_W, h: MIN_H, scale: 1 });

// The text scales in these steps, once the widest notch is also too short for a line.
const SCALE_STEPS = [0.95, 0.9, 0.85, 0.8, 0.75, 0.7];
const SCALE_FLOOR = 0.7;

/** How many rows a text takes at a width, wrapping at spaces and, for a longer word, by character. */
function countRows(text: string, px: number, weight: number, width: number, measure: Measure): number {
  const words = text.split(/\s+/).filter((word) => word.length > 0);
  if (words.length === 0) return 1;
  const space = measure(" ", px, weight);
  let rows = 1;
  let used = 0;
  for (const word of words) {
    const wordWidth = measure(word, px, weight);
    const join = used > 0 ? space : 0;
    if (used + join + wordWidth <= width) {
      used += join + wordWidth;
      continue;
    }
    if (used > 0) {
      rows += 1;
      used = 0;
    }
    if (wordWidth <= width) {
      used = wordWidth;
      continue;
    }
    // A word wider than the column breaks by character (overflow-wrap: anywhere).
    const full = Math.ceil(wordWidth / width);
    rows += full - 1;
    used = wordWidth - (full - 1) * width;
  }
  return rows;
}

/**
 * The height the body needs for the stack with `lines[index]` as the current line, at a width and
 * text scale. The neighbours are the lines before and after it, as the stack shows them.
 */
function stackNeed(lines: readonly string[], index: number, width: number, scale: number, measure: Measure): number {
  const avail = width - STACK_CHROME_W;
  const rows = (text: string, face: typeof CUR | typeof CTX) =>
    countRows(text, face.px * scale, face.weight, avail, measure);
  const cur = Math.max(CUR.minRows, rows(lines[index], CUR)) * CUR.line;
  const prev = lines[index - 1] === undefined ? 1 : Math.max(CTX.minRows, rows(lines[index - 1], CTX));
  const next = lines[index + 1] === undefined ? 1 : Math.max(CTX.minRows, rows(lines[index + 1], CTX));
  return (cur + (prev + next) * CTX.line) * scale + STACK_GAP;
}

/** The body height, at a width and scale, that holds every line of the track whole. */
function bodyNeed(lines: readonly string[], width: number, scale: number, measure: Measure): number {
  let need = 0;
  for (let i = 0; i < lines.length; i += 1) {
    need = Math.max(need, stackNeed(lines, i, width, scale, measure));
  }
  return need;
}

/** The lyrics pill's size for a track, computed from all its lines so it holds still as they change. */
export function fitTrack(lines: string[], measure: Measure): LyricsFit {
  if (lines.length === 0) return { ...DEFAULT_FIT };

  // 1. Widen from the rest size toward the largest, at the rest height.
  for (let w = MIN_W; w <= MAX_W; w += 1) {
    if (bodyNeed(lines, w, 1, measure) + CHROME_H <= MIN_H) {
      return { w, h: MIN_H, scale: 1 };
    }
  }

  // 2. At the widest, grow the height toward the largest, to hold the extra rows.
  const tall = bodyNeed(lines, MAX_W, 1, measure) + CHROME_H;
  if (tall <= MAX_H) return { w: MAX_W, h: Math.max(MIN_H, Math.ceil(tall)), scale: 1 };

  // 3. Only then shrink the text, one step at a time, and never below the floor.
  for (const scale of SCALE_STEPS) {
    const need = bodyNeed(lines, MAX_W, scale, measure) + CHROME_H;
    if (need <= MAX_H) return { w: MAX_W, h: Math.max(MIN_H, Math.ceil(need)), scale };
  }
  return { w: MAX_W, h: MAX_H, scale: SCALE_FLOOR };
}
