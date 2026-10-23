// The month grid's rules. Pure: the cells, the day keys, the note counts, the keyboard steps and the
// labels are tested without a window. A day is a "YYYY-MM-DD" string in local time, the form the
// backend uses. The grid always has six rows, so its height never changes between months.

import { datePart, type EventInstance } from "./calendar.ts";

export const GRID_ROWS = 6;
export const GRID_COLUMNS = 7;

const MONTH_NAMES = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];
const WEEKDAY_NAMES = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const WEEKDAY_SHORT = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/** One cell of the grid. `inMonth` is false for the days that belong to the months either side. */
export interface GridDay {
  key: string;
  day: number;
  inMonth: boolean;
}

/** A year and a month, with the month counted from 0 (January) as `Date` counts it. */
export interface YearMonth {
  year: number;
  month: number;
}

/** A step from the keyboard. Up and down move a week, the page keys move a month. */
export type Step = "left" | "right" | "up" | "down" | "home" | "end" | "pageUp" | "pageDown";

interface WeekInfo {
  firstDay?: number;
}

// Engines name the week data differently: current Chromium has the getWeekInfo() method, and older
// ones have a weekInfo property. Both are read, the method first.
interface LocaleWithWeek {
  getWeekInfo?: () => WeekInfo;
  weekInfo?: WeekInfo;
}

/**
 * The first column of the grid, 0 = Sunday to 6 = Saturday, from the locale's week. `Intl.Locale`
 * gives the first day as 1 (Monday) to 7 (Sunday). When the locale has no answer, the week starts on
 * Monday.
 */
export function firstWeekday(locale: string): number {
  try {
    const tag = new Intl.Locale(locale) as LocaleWithWeek;
    const info = tag.getWeekInfo ? tag.getWeekInfo() : tag.weekInfo;
    const first = info?.firstDay;
    if (typeof first === "number" && Number.isInteger(first) && first >= 1 && first <= 7) {
      return first % 7;
    }
  } catch {
    // An unknown or empty tag is not an error here: Monday is the answer.
  }
  return 1;
}

/** The short weekday names in grid order, starting at `firstDay`. */
export function weekdayHeaders(firstDay: number): string[] {
  return Array.from({ length: GRID_COLUMNS }, (_, i) => WEEKDAY_SHORT[(firstDay + i) % GRID_COLUMNS]);
}

/** The "YYYY-MM-DD" key for a year, a 0-based month and a day. */
export function dayKey(year: number, month: number, day: number): string {
  const date = new Date(Date.UTC(year, month, day));
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`;
}

/** The year and 0-based month a key falls in. */
export function monthOf(key: string): YearMonth {
  const date = utcDate(key);
  return { year: date.getUTCFullYear(), month: date.getUTCMonth() };
}

/** The day of the month of a key, 1 to 31. */
export function dayOf(key: string): number {
  return utcDate(key).getUTCDate();
}

/** The number of days in a month. February of a leap year has 29. */
export function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month + 1, 0)).getUTCDate();
}

/** The key `days` days after `key`. Negative counts go back. Crosses months and years. */
export function addDays(key: string, days: number): string {
  const date = utcDate(key);
  return dayKey(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate() + days);
}

/** The month `delta` months from `year` and `month`, across years. */
export function shiftMonth(year: number, month: number, delta: number): YearMonth {
  const index = year * 12 + month + delta;
  return { year: Math.floor(index / 12), month: ((index % 12) + 12) % 12 };
}

/** The same day in the month `delta` months away. A day the month lacks is clamped to its last day. */
export function shiftKeyByMonth(key: string, delta: number): string {
  const { year, month } = monthOf(key);
  const target = shiftMonth(year, month, delta);
  const day = Math.min(dayOf(key), daysInMonth(target.year, target.month));
  return dayKey(target.year, target.month, day);
}

/**
 * The six weeks shown for a month. The first cell is the first day of the week that holds the 1st,
 * with `firstDay` (0 = Sunday) as the week's start. Every month gets six rows, so a short month
 * shows the start of the next one, and the height never changes.
 */
export function monthGrid(year: number, month: number, firstDay: number): GridDay[][] {
  const first = dayKey(year, month, 1);
  const offset = (utcDate(first).getUTCDay() - firstDay + GRID_COLUMNS) % GRID_COLUMNS;
  const start = addDays(first, -offset);
  const rows: GridDay[][] = [];
  for (let row = 0; row < GRID_ROWS; row++) {
    const cells: GridDay[] = [];
    for (let column = 0; column < GRID_COLUMNS; column++) {
      const key = addDays(start, row * GRID_COLUMNS + column);
      const date = monthOf(key);
      cells.push({
        key,
        day: dayOf(key),
        inMonth: date.year === year && date.month === month,
      });
    }
    rows.push(cells);
  }
  return rows;
}

/** The first and last key of a grid, both included. These are the days to ask the backend for. */
export function gridRange(rows: GridDay[][]): { from: string; to: string } {
  return { from: rows[0][0].key, to: rows[rows.length - 1][GRID_COLUMNS - 1].key };
}

/** "October 2026", the title above the grid. */
export function monthTitle(year: number, month: number): string {
  return `${MONTH_NAMES[month]} ${year}`;
}

/**
 * How many notes start on each day. A note is counted on the day it starts, so an event that runs
 * past midnight counts once. Days with no notes are absent from the map.
 */
export function noteCounts(instances: EventInstance[]): Map<string, number> {
  const counts = new Map<string, number>();
  for (const instance of instances) {
    const key = datePart(instance.start);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

/** The notes that start on `key`, in the order the backend gave them. */
export function instancesOn(instances: EventInstance[], key: string): EventInstance[] {
  return instances.filter((instance) => datePart(instance.start) === key);
}

/** "Friday 23 October", for a key. */
export function longDate(key: string): string {
  const date = utcDate(key);
  return `${WEEKDAY_NAMES[date.getUTCDay()]} ${date.getUTCDate()} ${MONTH_NAMES[date.getUTCMonth()]}`;
}

/** "1 note" or "2 notes", and "No notes" for none. */
export function noteCountLabel(count: number): string {
  if (count === 0) return "No notes";
  return count === 1 ? "1 note" : `${count} notes`;
}

/** The label a grid cell has for screen readers: "Friday 23 October, 2 notes". */
export function cellLabel(key: string, count: number): string {
  return `${longDate(key)}, ${noteCountLabel(count).toLowerCase()}`;
}

/** The key one step from `key`. The week keys stay in the same week row, and the page keys keep the day. */
export function stepKey(key: string, step: Step, firstDay: number): string {
  switch (step) {
    case "left":
      return addDays(key, -1);
    case "right":
      return addDays(key, 1);
    case "up":
      return addDays(key, -GRID_COLUMNS);
    case "down":
      return addDays(key, GRID_COLUMNS);
    case "home": {
      const column = (utcDate(key).getUTCDay() - firstDay + GRID_COLUMNS) % GRID_COLUMNS;
      return addDays(key, -column);
    }
    case "end": {
      const column = (utcDate(key).getUTCDay() - firstDay + GRID_COLUMNS) % GRID_COLUMNS;
      return addDays(key, GRID_COLUMNS - 1 - column);
    }
    case "pageUp":
      return shiftKeyByMonth(key, -1);
    case "pageDown":
      return shiftKeyByMonth(key, 1);
  }
}

/**
 * The time a new note starts at: the next full hour from `now`, as HH:00. Just before midnight the
 * next hour is 00:00, which is the start of the day.
 */
export function defaultTime(now: Date): string {
  return `${pad((now.getHours() + 1) % 24)}:00`;
}

function utcDate(key: string): Date {
  const [year, month, day] = key.split("-").map(Number);
  return new Date(Date.UTC(year, month - 1, day));
}

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

/** The rows the island's notes column has: its 102 px body holds four 24 px rows. */
export const NOTE_ROWS = 4;

/**
 * How the island's notes column shows `count` notes in `rows` slots. Every note is shown when they fit.
 * Otherwise the last slot becomes "+N more", so one slot is given up and `more` counts what it hides.
 */
export function noteRows(count: number, rows = NOTE_ROWS): { shown: number; more: number } {
  if (count <= rows) return { shown: count, more: 0 };
  const shown = rows - 1;
  return { shown, more: count - shown };
}
