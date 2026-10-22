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
