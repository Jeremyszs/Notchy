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

/**
 * The column's width is less this many px when a line is counted. The browser wraps the real text, and
 * a line that is exactly as wide as the column can wrap one word more than the measure says. Counting
 * with a few px to spare keeps the stack inside the body (measured: an exact fit left 0 px of slack).
 */
export const WRAP_SAFETY_PX = 4;

// Characters a line may break after without a space: CJK, kana, hangul and fullwidth forms.
const BREAKS_WITHOUT_SPACE = /[\u2e80-\u303f\u3040-\u9fff\uac00-\ud7af\uf900-\ufaff\uff00-\uffef]/;

type Token = { text: string; spaceBefore: boolean };

/**
 * Splits a text into the pieces the browser may wrap between: space-separated words, and each CJK
 * character on its own. A piece remembers whether a space came before it, since a row that breaks
 * there drops the space.
 */
function tokenize(text: string): Token[] {
  const tokens: Token[] = [];
  let space = false;
  let word = "";
  let wordSpace = false;
  for (const ch of text) {
    if (/\s/.test(ch)) {
      if (word) tokens.push({ text: word, spaceBefore: wordSpace });
      word = "";
      space = true;
    } else if (BREAKS_WITHOUT_SPACE.test(ch)) {
      if (word) tokens.push({ text: word, spaceBefore: wordSpace });
      word = "";
      tokens.push({ text: ch, spaceBefore: space });
      space = false;
    } else {
      if (!word) {
        wordSpace = space;
        space = false;
      }
      word += ch;
    }
  }
  if (word) tokens.push({ text: word, spaceBefore: wordSpace });
  return tokens;
}

/**
 * How many rows a text takes in a column of `width` px, after the safety margin. Pieces wrap as the
 * browser wraps them: a piece that does not fit moves to a new row, and a piece wider than a row breaks
 * between characters, filling each row in turn (overflow-wrap: anywhere). Exported for the tests.
 */
export function countRows(text: string, px: number, weight: number, width: number, measure: Measure): number {
  const limit = width - WRAP_SAFETY_PX;
  const tokens = tokenize(text);
  if (tokens.length === 0) return 1;
  const space = measure(" ", px, weight);
  let rows = 1;
  let used = 0;
  for (const token of tokens) {
    const pieceWidth = measure(token.text, px, weight);
    const join = used > 0 && token.spaceBefore ? space : 0;
    if (used + join + pieceWidth <= limit) {
      used += join + pieceWidth;
      continue;
    }
    if (used > 0) {
      rows += 1;
      used = 0;
    }
    if (pieceWidth <= limit) {
      used = pieceWidth;
      continue;
    }
    // Wider than a row: each row takes the characters that fit, and the next one starts when one does not.
    for (const ch of token.text) {
      const chWidth = measure(ch, px, weight);
      if (used > 0 && used + chWidth > limit) {
        rows += 1;
        used = 0;
      }
      used += chWidth;
    }
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
