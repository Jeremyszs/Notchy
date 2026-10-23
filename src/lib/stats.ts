// The Stats page's view rules. Pure, so the units, the figures and the sparkline are tested without a
// window. The types mirror StatsSnapshot in src-tauri/src/modules/stats.rs.

export interface StatsSnapshot {
  /** Busy share of the CPU, 0 to 100. Null until there are two readings. */
  cpu: number | null;
  memory: { used: number; total: number; percent: number };
  /** Bytes per second. Null until there are two readings. */
  network: { down: number | null; up: number | null };
  disk: { total: number; free: number; percent: number };
}

/** A figure split into its digits and its unit, so the digits can roll and the unit stays put. */
export interface Figure {
  /** The digits, or null before the first reading. */
  number: string | null;
  unit: string;
}

/** How many samples a sparkline keeps. At one a second, that is half a minute. */
export const HISTORY = 30;

/** A sparkline's network scale never goes below this, so idle noise does not look like a spike. */
export const NETWORK_FLOOR = 64 * 1024;

const KIB = 1024;
const GIB = KIB ** 3;

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

/** A percentage as whole digits, such as "62". Null before the first reading. */
export function percentFigure(percent: number | null): Figure {
  if (percent === null || !Number.isFinite(percent)) return { number: null, unit: "%" };
  return { number: String(Math.round(clamp(percent, 0, 100))), unit: "%" };
}

/** A byte rate in KB/s, MB/s or GB/s, with one decimal above KB/s. Null before the first reading. */
export function rateFigure(bytesPerSecond: number | null): Figure {
  if (bytesPerSecond === null || !Number.isFinite(bytesPerSecond)) {
    return { number: null, unit: "" };
  }
  const kb = Math.max(0, bytesPerSecond) / KIB;
  if (Math.round(kb) < 1000) return { number: String(Math.round(kb)), unit: "KB/s" };
  const mb = kb / KIB;
  // Up to 999.9 MB/s. The next step up is shown in GB/s, so the number never grows a digit.
  if (mb < 999.95) return { number: mb.toFixed(1), unit: "MB/s" };
  return { number: (mb / KIB).toFixed(1), unit: "GB/s" };
}

/** A rate as one line of text, such as "1.1 MB/s", or a dash before the first reading. */
export function rateText(bytesPerSecond: number | null): string {
  const figure = rateFigure(bytesPerSecond);
  return figure.number === null ? "–" : `${figure.number} ${figure.unit}`;
}

/** Bytes as gibibytes with one decimal, such as "16.0". */
export function gigabytes(bytes: number): string {
  return (Math.max(0, bytes) / GIB).toFixed(1);
}

/** The memory detail line, such as "7.4 of 16.0 GB". */
export function memoryDetail(memory: { used: number; total: number }): string {
  return `${gigabytes(memory.used)} of ${gigabytes(memory.total)} GB`;
}

/** The disk detail line, such as "212.5 GB free". */
export function diskDetail(disk: { free: number }): string {
  return `${gigabytes(disk.free)} GB free`;
}

/** Adds a sample to a history, dropping the oldest past HISTORY. A missing sample is not kept. */
export function pushSample(history: number[], value: number | null): number[] {
  if (value === null || !Number.isFinite(value)) return history;
  const next = [...history, value];
  return next.length > HISTORY ? next.slice(next.length - HISTORY) : next;
}

/**
 * The top of a sparkline: the largest sample, but never below `floor`. The newest sample sits at
 * the right, so the scale only moves when a larger value arrives.
 */
export function sparkMax(values: number[], floor: number): number {
  return Math.max(floor, ...values);
}

/**
 * The SVG path of a sparkline in a `width` x `height` box. Values run from 0 at the bottom to `max`
 * at the top. The newest value sits at the right edge and the line grows in from the right, so it
 * keeps the same horizontal spacing while the history fills. Empty for fewer than two values.
 */
export function sparkPath(
  values: number[],
  width: number,
  height: number,
  max: number,
  capacity = HISTORY,
): string {
  if (values.length < 2 || !(max > 0)) return "";
  const step = width / (capacity - 1);
  const first = capacity - values.length;
  return values
    .map((value, i) => {
      const x = (first + i) * step;
      const y = height - (clamp(value, 0, max) / max) * height;
      return `${i === 0 ? "M" : "L"}${round(x)} ${round(y)}`;
    })
    .join(" ");
}

function round(value: number): string {
  return String(Math.round(value * 100) / 100);
}
