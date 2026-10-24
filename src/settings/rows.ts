// The row model behind the settings editors. Pure helpers, so the ordering rules stay out of the markup.

export interface Row {
  id: string;
  label: string;
  /** Whether the item is in the notch. Shown ids are the ones that are on. */
  on: boolean;
  /** Whether the item can be added or removed right now. */
  enabled: boolean;
  /** Dims the item, for a control this machine cannot run or a page whose feature is off. */
  dim?: boolean;
  /** A short line saying why the item is dimmed or locked. */
  note?: string;
}

/** Every known id in list order: the shown ones first, in their shown order, then the rest. */
export function ordered(known: string[], shown: string[]): string[] {
  return [
    ...shown.filter((id) => known.includes(id)),
    ...known.filter((id) => !shown.includes(id)),
  ];
}

/** Moves `id` to index `to` in a list of ids. Returns the same array when nothing would move. */
export function moveId(ids: string[], id: string, to: number): string[] {
  const from = ids.indexOf(id);
  if (from < 0 || to < 0 || to >= ids.length || from === to) return ids;
  const next = ids.slice();
  next.splice(from, 1);
  next.splice(to, 0, id);
  return next;
}

/**
 * The rows with `shown` on, in that order, followed by every other row switched off in its
 * current order. Ids in `shown` that no row has are ignored.
 */
export function arrange(rows: Row[], shown: string[]): Row[] {
  const byId = new Map(rows.map((row) => [row.id, row]));
  const first = shown.flatMap((id) => {
    const row = byId.get(id);
    return row ? [{ ...row, on: true }] : [];
  });
  const rest = rows.filter((row) => !shown.includes(row.id)).map((row) => ({ ...row, on: false }));
  return [...first, ...rest];
}

/** Adds a row to the end of the shown ones, or takes it out. */
export function showRow(rows: Row[], id: string, on: boolean): Row[] {
  const shown = rows.filter((row) => row.on && row.id !== id).map((row) => row.id);
  return arrange(rows, on ? [...shown, id] : shown);
}

/** The index of the slot whose centre is nearest the point. -1 when there are no slots. */
export function nearestSlot(slots: { x: number; y: number }[], px: number, py: number): number {
  let best = -1;
  let bestDistance = Infinity;
  slots.forEach((slot, index) => {
    const distance = (slot.x - px) ** 2 + (slot.y - py) ** 2;
    if (distance < bestDistance) {
      bestDistance = distance;
      best = index;
    }
  });
  return best;
}
