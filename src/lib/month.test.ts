import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addDays,
  cellLabel,
  daysInMonth,
  dayKey,
  dayOf,
  defaultTime,
  firstWeekday,
  gridRange,
  instancesOn,
  longDate,
  monthGrid,
  monthOf,
  monthTitle,
  noteCountLabel,
  noteRows,
  noteCounts,
  shiftKeyByMonth,
  shiftMonth,
  stepKey,
  weekdayHeaders,
  type GridDay,
} from "./month.ts";
import type { EventInstance } from "./calendar.ts";

function instance(start: string, title = "Note"): EventInstance {
  return {
    eventId: title,
    source: "local",
    title,
    notes: "",
    start,
    end: start,
    allDay: false,
    readOnly: false,
    remindBeforeMinutes: null,
  };
}

function keys(rows: GridDay[][]): string[] {
  return rows.flat().map((cell) => cell.key);
}

test("the first weekday comes from the locale, and Monday when it is unknown", () => {
  assert.equal(firstWeekday("en-US"), 0);
  assert.equal(firstWeekday("en-GB"), 1);
  assert.equal(firstWeekday("de-DE"), 1);
  assert.equal(firstWeekday(""), 1);
  assert.equal(firstWeekday("not a locale!!"), 1);
});

test("weekday headers start on the chosen day", () => {
  assert.deepEqual(weekdayHeaders(1), ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]);
  assert.deepEqual(weekdayHeaders(0), ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]);
});

test("October 2026 with Monday first starts on the Monday 28 September", () => {
  // 1 October 2026 is a Thursday, so three days of September come before it.
  const rows = monthGrid(2026, 9, 1);
  assert.equal(rows.length, 6);
  assert.ok(rows.every((row) => row.length === 7));
  assert.equal(rows[0][0].key, "2026-09-28");
  assert.equal(rows[0][0].inMonth, false);
  const first = rows[0].find((cell) => cell.inMonth);
  assert.equal(first?.key, "2026-10-01");
  assert.deepEqual(gridRange(rows), { from: "2026-09-28", to: "2026-11-08" });
});

test("the same month with Sunday first starts on the Sunday 27 September", () => {
  const rows = monthGrid(2026, 9, 0);
  assert.equal(rows[0][0].key, "2026-09-27");
  assert.equal(rows[0][4].key, "2026-10-01");
  assert.equal(rows[0][4].inMonth, true);
});

test("February 2028 is a leap month with 29 days in the grid", () => {
  assert.equal(daysInMonth(2028, 1), 29);
  assert.equal(daysInMonth(2027, 1), 28);
  const rows = monthGrid(2028, 1, 1);
  const inMonth = rows.flat().filter((cell) => cell.inMonth);
  assert.equal(inMonth.length, 29);
  assert.equal(inMonth.at(-1)?.key, "2028-02-29");
  assert.equal(inMonth.at(-1)?.day, 29);
  assert.equal(rows.length, 6);
});

test("February 2027 starts on a Monday, fills four weeks, and still has six rows", () => {
  const rows = monthGrid(2027, 1, 1);
  assert.equal(rows.length, 6);
  assert.equal(rows[0][0].key, "2027-02-01");
  // Four full weeks hold February, so the last two rows are all March, dimmed.
  assert.equal(rows[4][0].key, "2027-03-01");
  assert.ok(rows[4].every((cell) => !cell.inMonth));
  assert.ok(rows[5].every((cell) => !cell.inMonth));
  assert.equal(rows[5][0].key, "2027-03-08");
});

test("every month gets six rows of seven, whatever its first day", () => {
  for (let month = 0; month < 12; month++) {
    for (const firstDay of [0, 1]) {
      const rows = monthGrid(2026, month, firstDay);
      assert.equal(rows.length, 6, `month ${month}`);
      assert.ok(rows.every((row) => row.length === 7), `month ${month}`);
      const inMonth = rows.flat().filter((cell) => cell.inMonth).length;
      assert.equal(inMonth, daysInMonth(2026, month), `month ${month}`);
    }
  }
});

test("day keys are padded, and dates add and subtract across months and years", () => {
  assert.equal(dayKey(2026, 0, 5), "2026-01-05");
  assert.equal(addDays("2026-12-31", 1), "2027-01-01");
  assert.equal(addDays("2027-01-01", -1), "2026-12-31");
  assert.equal(addDays("2028-02-28", 1), "2028-02-29");
  assert.equal(addDays("2027-02-28", 1), "2027-03-01");
  assert.deepEqual(monthOf("2026-10-23"), { year: 2026, month: 9 });
  assert.equal(dayOf("2026-10-23"), 23);
});

test("month navigation crosses years and keeps the day where it can", () => {
  assert.deepEqual(shiftMonth(2026, 11, 1), { year: 2027, month: 0 });
  assert.deepEqual(shiftMonth(2026, 0, -1), { year: 2025, month: 11 });
  assert.deepEqual(shiftMonth(2026, 1, 12), { year: 2027, month: 1 });
  assert.deepEqual(shiftMonth(2026, 1, -14), { year: 2024, month: 11 });
  assert.equal(shiftKeyByMonth("2026-10-23", 1), "2026-11-23");
  assert.equal(shiftKeyByMonth("2026-01-31", 1), "2026-02-28");
  assert.equal(shiftKeyByMonth("2028-01-31", 1), "2028-02-29");
  assert.equal(monthTitle(2026, 9), "October 2026");
});

test("note counts are keyed by the day each note starts, and an empty list gives none", () => {
  const counts = noteCounts([
    instance("2026-10-23T09:00", "a"),
    instance("2026-10-23T18:00", "b"),
    instance("2026-10-24T00:00", "c"),
  ]);
  assert.equal(counts.get("2026-10-23"), 2);
  assert.equal(counts.get("2026-10-24"), 1);
  assert.equal(counts.has("2026-10-25"), false);
  assert.equal(noteCounts([]).size, 0);
});

test("the notes for one day keep the order they came in", () => {
  const list = [instance("2026-10-23T09:00", "a"), instance("2026-10-24T09:00", "b"), instance("2026-10-23T18:00", "c")];
  assert.deepEqual(
    instancesOn(list, "2026-10-23").map((item) => item.title),
    ["a", "c"],
  );
  assert.deepEqual(instancesOn(list, "2026-10-30"), []);
});

test("labels read the date in full and say how many notes there are", () => {
  assert.equal(longDate("2026-10-23"), "Friday 23 October");
  assert.equal(noteCountLabel(0), "No notes");
  assert.equal(noteCountLabel(1), "1 note");
  assert.equal(noteCountLabel(3), "3 notes");
  assert.equal(cellLabel("2026-10-23", 2), "Friday 23 October, 2 notes");
  assert.equal(cellLabel("2026-10-23", 1), "Friday 23 October, 1 note");
  assert.equal(cellLabel("2026-10-23", 0), "Friday 23 October, no notes");
});

test("keyboard steps move a day, a week or a month, and Home and End stay in the row", () => {
  assert.equal(stepKey("2026-10-01", "left", 1), "2026-09-30");
  assert.equal(stepKey("2026-10-31", "right", 1), "2026-11-01");
  assert.equal(stepKey("2026-10-05", "up", 1), "2026-09-28");
  assert.equal(stepKey("2026-10-28", "down", 1), "2026-11-04");
  // 23 October 2026 is a Friday. Monday-first, its row starts on Monday 19 and ends on Sunday 25.
  assert.equal(stepKey("2026-10-23", "home", 1), "2026-10-19");
  assert.equal(stepKey("2026-10-23", "end", 1), "2026-10-25");
  // Sunday-first, the same Friday's row starts on Sunday 18 and ends on Saturday 24.
  assert.equal(stepKey("2026-10-23", "home", 0), "2026-10-18");
  assert.equal(stepKey("2026-10-23", "end", 0), "2026-10-24");
  assert.equal(stepKey("2026-10-31", "pageDown", 1), "2026-11-30");
  assert.equal(stepKey("2026-03-31", "pageUp", 1), "2026-02-28");
});

test("the default time is the next full hour, and it wraps at midnight", () => {
  assert.equal(defaultTime(new Date(2026, 9, 23, 14, 35)), "15:00");
  assert.equal(defaultTime(new Date(2026, 9, 23, 0, 0)), "01:00");
  assert.equal(defaultTime(new Date(2026, 9, 23, 23, 50)), "00:00");
});

test("the notes column shows every note that fits, then a +N more row", () => {
  assert.deepEqual(noteRows(0), { shown: 0, more: 0 });
  assert.deepEqual(noteRows(1), { shown: 1, more: 0 });
  assert.deepEqual(noteRows(4), { shown: 4, more: 0 });
  assert.deepEqual(noteRows(5), { shown: 3, more: 2 });
  assert.deepEqual(noteRows(6), { shown: 3, more: 3 });
  assert.deepEqual(noteRows(3, 2), { shown: 1, more: 2 });
});
