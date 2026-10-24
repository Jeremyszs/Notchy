export interface ShelfItem {
  id: number;
  kind: "text" | "file";
  preview?: string;
  name?: string;
  path?: string;
  ext?: string;
  pinned: boolean;
  atMs: number;
  missing?: boolean;
}

export function relativeAge(atMs: number, nowMs: number): string {
  const minutes = Math.max(0, Math.floor((nowMs - atMs) / 60_000));
  if (minutes === 0) return "now";
  if (minutes < 60) return `${minutes}m`;
  if (minutes < 1440) return `${Math.floor(minutes / 60)}h`;
  return `${Math.floor(minutes / 1440)}d`;
}

export function fileName(path: string): string {
  return path.replace(/[\\/]+$/, "").split(/[\\/]/).pop() || path;
}

export function extension(name: string, folder = false): string {
  const dot = name.lastIndexOf(".");
  return folder || dot <= 0 ? "" : name.slice(dot + 1);
}

/** The strip owns every wheel event, even at an end or without overflow. */
export function wheelRoute(insideStrip: boolean, deltaX: number, deltaY: number) {
  return { consumed: insideStrip, delta: insideStrip ? (Math.abs(deltaX) > Math.abs(deltaY) ? deltaX : deltaY) : 0 };
}

/** A CSS cubic-bezier curve as a function of progress, so a Svelte transition matches the CSS one. */
export function cubicBezier(x1: number, y1: number, x2: number, y2: number): (x: number) => number {
  const axis = (a: number, b: number, t: number) => 3 * a * (1 - t) ** 2 * t + 3 * b * (1 - t) * t ** 2 + t ** 3;
  return (x) => {
    // Find the curve parameter whose x matches the input, then read y there.
    let low = 0;
    let high = 1;
    let t = x;
    for (let i = 0; i < 30; i++) {
      t = (low + high) / 2;
      if (axis(x1, x2, t) < x) low = t;
      else high = t;
    }
    return axis(y1, y2, t);
  };
}

export type ShelfMode = "file" | "text";

/** Files when files are on, or when both are on; Text only when clipboard history is the one on. */
export function defaultMode(clipboard: boolean, files: boolean): ShelfMode {
  return clipboard && !files ? "text" : "file";
}

/** A press becomes a drag once the pointer has moved this far, so a click never starts one. */
export const DRAG_START_PX = 6;

export function movedBeyond(fromX: number, fromY: number, x: number, y: number, px: number): boolean {
  return Math.hypot(x - fromX, y - fromY) > px;
}
