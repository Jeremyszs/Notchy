// The row model behind the settings lists. Pure helpers, so the ordering rules stay out of the markup.

/** The height of one list row in px. ReorderList moves rows in steps of this size. */
export const ROW_HEIGHT = 56;

export interface Row {
  id: string;
  label: string;
  /** The switch state. Shown ids are the ones that are on. */
  on: boolean;
  /** Whether the switch can be operated right now. */
  enabled: boolean;
  /** Dims the row, for a control this machine cannot run. */
  dim?: boolean;
  /** A short line under the label, such as why a control is unavailable. */
  note?: string;
}

/** Every known id in list order: the shown ones first, in their shown order, then the rest. */
export function ordered(known: string[], shown: string[]): string[] {
  return [
    ...shown.filter((id) => known.includes(id)),
    ...known.filter((id) => !shown.includes(id)),
  ];
}

/** Moves the row with `id` to index `to`. Returns the same array when nothing would move. */
export function moveRow(rows: Row[], id: string, to: number): Row[] {
  const from = rows.findIndex((row) => row.id === id);
  if (from < 0 || to < 0 || to >= rows.length || from === to) return rows;
  const next = rows.slice();
  const [row] = next.splice(from, 1);
  next.splice(to, 0, row);
  return next;
}

/** A row drag in progress. `from` and `to` are indexes; `dy` is the pointer travel in px. */
export interface Drag {
  id: string;
  from: number;
  to: number;
  dy: number;
}

/** A row that was just dropped. It keeps `y` px of offset until it glides into its slot. */
export interface Settle {
  id: string;
  y: number;
}

/**
 * The vertical offset in px of the row `id` at `index`. A drag wins: the dragged row follows the
 * pointer and the rows between its start and drop point step aside by one row. Otherwise only the
 * settling row is offset.
 */
export function rowOffset(
  id: string,
  index: number,
  drag: Drag | null,
  settle: Settle | null,
): number {
  if (drag) {
    if (id === drag.id) return drag.dy;
    if (index > drag.from && index <= drag.to) return -ROW_HEIGHT;
    if (index < drag.from && index >= drag.to) return ROW_HEIGHT;
    return 0;
  }
  return settle && id === settle.id ? settle.y : 0;
}

/**
 * The offset a dropped row keeps so it stays under the pointer in its new slot. Rows are
 * ROW_HEIGHT apart, so the offset is what the pointer travelled beyond the whole rows it moved.
 */
export function settleOffset(dy: number, from: number, to: number): number {
  return dy - (to - from) * ROW_HEIGHT;
}

/** The drop slot for a pointer that moved `dy` px from row `from`, clamped to the list. */
export function dropIndex(from: number, dy: number, length: number): number {
  const slots = Math.round(dy / ROW_HEIGHT);
  return Math.min(Math.max(from + slots, 0), length - 1);
}
