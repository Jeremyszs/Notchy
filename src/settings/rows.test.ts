import { test } from "node:test";
import assert from "node:assert/strict";
import { arrange, moveId, nearestSlot, ordered, showRow, type Row } from "./rows.ts";

const row = (id: string, on: boolean): Row => ({ id, label: id.toUpperCase(), on, enabled: true });

test("ordered keeps shown ids first in shown order, then the rest", () => {
  assert.deepEqual(ordered(["a", "b", "c", "d"], ["c", "a"]), ["c", "a", "b", "d"]);
  assert.deepEqual(ordered(["a", "b"], ["z", "b"]), ["b", "a"]);
});

test("moveId moves an id and returns the same array when nothing moves", () => {
  const ids = ["a", "b", "c"];
  assert.deepEqual(moveId(ids, "a", 2), ["b", "c", "a"]);
  assert.deepEqual(moveId(ids, "c", 0), ["c", "a", "b"]);
  assert.equal(moveId(ids, "b", 1), ids);
  assert.equal(moveId(ids, "x", 0), ids);
  assert.equal(moveId(ids, "a", 3), ids);
});

test("arrange puts the shown ids first in their order and switches the rest off", () => {
  const rows = [row("a", true), row("b", true), row("c", false), row("d", false)];
  const next = arrange(rows, ["c", "a"]);
  assert.deepEqual(next.map((r) => [r.id, r.on]), [["c", true], ["a", true], ["b", false], ["d", false]]);
  assert.equal(next[0].label, "C");
  assert.deepEqual(arrange(rows, ["zzz"]).map((r) => r.on), [false, false, false, false]);
});

test("showRow adds to the end of the shown rows and removes without moving the others", () => {
  const rows = [row("a", true), row("b", true), row("c", false)];
  assert.deepEqual(showRow(rows, "c", true).map((r) => [r.id, r.on]), [["a", true], ["b", true], ["c", true]]);
  assert.deepEqual(showRow(rows, "a", false).map((r) => [r.id, r.on]), [["b", true], ["a", false], ["c", false]]);
});

test("nearestSlot picks the closest centre and is -1 with no slots", () => {
  const slots = [{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 0, y: 100 }];
  assert.equal(nearestSlot(slots, 70, 10), 1);
  assert.equal(nearestSlot(slots, 10, 60), 2);
  assert.equal(nearestSlot([], 0, 0), -1);
});
