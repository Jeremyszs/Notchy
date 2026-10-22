// Song tint for the lyrics layout. The pixel and hash helpers are pure, so they can be checked
// without a DOM. Only tintFromImage touches the canvas.

/** Hue in degrees, saturation and lightness in percent. */
export interface Tint {
  h: number;
  s: number;
  l: number;
}

// Saturation and lightness bounds. The lightness cap keeps white text readable on the tint.
const MIN_S = 45;
const MAX_S = 75;
const MIN_L = 28;
const MAX_L = 40;
// Art is averaged at this size. 16 by 16 is enough to find the dominant colour cheaply.
const SAMPLE = 16;

const clamp = (value: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, value));

/** Converts 0..255 RGB to hue in degrees, and saturation and lightness in 0..1. */
function rgbToHsl(r: number, g: number, b: number): { h: number; s: number; l: number } {
  const red = r / 255;
  const green = g / 255;
  const blue = b / 255;
  const max = Math.max(red, green, blue);
  const min = Math.min(red, green, blue);
  const l = (max + min) / 2;
  const d = max - min;
  if (d === 0) return { h: 0, s: 0, l };

  const s = d / (1 - Math.abs(2 * l - 1));
  let h: number;
  if (max === red) h = ((green - blue) / d) % 6;
  else if (max === green) h = (blue - red) / d + 2;
  else h = (red - green) / d + 4;
  return { h: ((h * 60) + 360) % 360, s, l };
}

/**
 * The tint of RGBA pixels (4 values per pixel, as in ImageData.data). Near-black and near-white
 * pixels are skipped. Each other pixel votes for its hue with its saturation as the weight, so
 * a colourful area outweighs grey. Returns null when nothing colourful is left.
 */
export function tintFromPixels(rgba: ArrayLike<number>): Tint | null {
  let x = 0;
  let y = 0;
  let weight = 0;
  let sat = 0;
  let light = 0;
  let count = 0;

  for (let i = 0; i + 3 < rgba.length; i += 4) {
    const { h, s, l } = rgbToHsl(rgba[i], rgba[i + 1], rgba[i + 2]);
    if (l < 0.08 || l > 0.92) continue;
    count++;
    sat += s;
    light += l;
    if (s === 0) continue;
    const angle = (h * Math.PI) / 180;
    x += Math.cos(angle) * s;
    y += Math.sin(angle) * s;
    weight += s;
  }

  if (count === 0 || weight === 0) return null;

  const hue = (Math.atan2(y, x) * 180) / Math.PI;
  return {
    h: (hue + 360) % 360,
    s: clamp((sat / count) * 100, MIN_S, MAX_S),
    l: clamp((light / count) * 100, MIN_L, MAX_L),
  };
}

/** A stable tint for text. The same text always gives the same colour. */
export function tintFromText(text: string): string {
  // FNV-1a. Any stable hash works; this one is short and needs no table.
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  const hue = hash % 360;
  const s = MIN_S + ((hash >>> 9) % (MAX_S - MIN_S + 1));
  const l = MIN_L + ((hash >>> 17) % (MAX_L - MIN_L + 1));
  return cssColor({ h: hue, s, l });
}

export function cssColor({ h, s, l }: Tint): string {
  return `hsl(${Math.round(h)} ${Math.round(s)}% ${Math.round(l)}%)`;
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error("image did not decode"));
    img.src = src;
  });
}

/**
 * The CSS colour for a track. Reads the album art (a data URI) at 16 by 16. Without art, or when
 * decoding or reading the pixels fails (a tainted canvas, for example), it falls back to a hash of
 * the fallback text, usually "title artist".
 */
export async function tintFromImage(
  dataUri: string | null,
  fallbackText: string,
): Promise<string> {
  const fallback = tintFromText(fallbackText);
  if (!dataUri) return fallback;

  try {
    const img = await loadImage(dataUri);
    const canvas = document.createElement("canvas");
    canvas.width = SAMPLE;
    canvas.height = SAMPLE;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return fallback;
    ctx.drawImage(img, 0, 0, SAMPLE, SAMPLE);
    const { data } = ctx.getImageData(0, 0, SAMPLE, SAMPLE);
    const tint = tintFromPixels(data);
    return tint ? cssColor(tint) : fallback;
  } catch {
    return fallback;
  }
}
