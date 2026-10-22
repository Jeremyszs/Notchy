import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ROW_HEIGHT,
  dropIndex,
  moveRow,
  ordered,
  rowOffset,
  settleOffset,
  type Row,
} from "./rows.ts";

const row = (id: string): Row => ({ id, label: id, on: true, enabled: true });

test("ordered keeps shown ids first in shown order, then the rest", () => {
  assert.deepEqual(ordered(["a", "b", "c", "d"], ["c", "a"]), ["c", "a", "b", "d"]);
  assert.deepEqual(ordered(["a", "b"], ["z", "b"]), ["b", "a"]);
});

test("moveRow moves a row and returns the same array when nothing moves", () => {
  const rows = ["a", "b", "c"].map(row);
  assert.deepEqual(
    moveRow(rows, "a", 2).map((r) => r.id),
    ["b", "c", "a"],
  );
  assert.deepEqual(
    moveRow(rows, "c", 0).map((r) => r.id),
    ["c", "a", "b"],
  );
  assert.equal(moveRow(rows, "b", 1), rows);
  assert.equal(moveRow(rows, "b", 3), rows);
  assert.equal(moveRow(rows, "missing", 0), rows);
});

test("dropIndex snaps to whole rows and clamps to the list", () => {
  assert.equal(dropIndex(0, 0, 4), 0);
  assert.equal(dropIndex(0, ROW_HEIGHT * 0.4, 4), 0);
  assert.equal(dropIndex(0, ROW_HEIGHT * 0.6, 4), 1);
  assert.equal(dropIndex(1, -ROW_HEIGHT * 2, 4), 0);
  assert.equal(dropIndex(2, ROW_HEIGHT * 10, 4), 3);
});

test("a dragged row follows the pointer and rows in its path step aside", () => {
  // Row 0 dragged down to index 2: rows 1 and 2 step up one row, row 3 stays.
  const drag = { id: "a", from: 0, to: 2, dy: 100 };
  assert.equal(rowOffset("a", 0, drag, null), 100);
  assert.equal(rowOffset("b", 1, drag, null), -ROW_HEIGHT);
  assert.equal(rowOffset("c", 2, drag, null), -ROW_HEIGHT);
  assert.equal(rowOffset("d", 3, drag, null), 0);
});

test("a row dragged up steps down past the rows it passes", () => {
  const drag = { id: "c", from: 2, to: 0, dy: -110 };
  assert.equal(rowOffset("a", 0, drag, null), ROW_HEIGHT);
  assert.equal(rowOffset("b", 1, drag, null), ROW_HEIGHT);
  assert.equal(rowOffset("c", 2, drag, null), -110);
  assert.equal(rowOffset("d", 3, drag, null), 0);
});

test("with no drag and no settle every row sits in its slot", () => {
  assert.equal(rowOffset("a", 0, null, null), 0);
});

test("the settling row keeps its offset and the others are at rest", () => {
  const settle = { id: "b", y: -26 };
  assert.equal(rowOffset("b", 1, null, settle), -26);
  assert.equal(rowOffset("a", 0, null, settle), 0);
  assert.equal(rowOffset("c", 2, null, settle), 0);
});

test("a new drag takes precedence over a row still settling", () => {
  const settle = { id: "b", y: -26 };
  const drag = { id: "c", from: 2, to: 2, dy: 5 };
  assert.equal(rowOffset("b", 1, drag, settle), 0);
  assert.equal(rowOffset("c", 2, drag, settle), 5);
});

test("settleOffset keeps the row under the pointer in its new slot", () => {
  // Moved 30px down: lands one slot lower, so it sits 26px above its slot and glides down.
  assert.equal(settleOffset(30, 0, 1), 30 - ROW_HEIGHT);
  // Moved exactly two rows: no residual offset.
  assert.equal(settleOffset(2 * ROW_HEIGHT, 0, 2), 0);
  // Moved up one row, 10px short of the slot: sits 10px below its slot.
  assert.equal(settleOffset(-ROW_HEIGHT + 10, 2, 1), 10);
});
