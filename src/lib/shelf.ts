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
