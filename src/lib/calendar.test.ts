import { test } from "node:test";
import assert from "node:assert/strict";
import {
  dateKey,
  dayLabel,
  describeEvent,
  emptyForm,
  formStart,
  hasErrors,
  hourLabel,
  isValidDate,
  isValidTime,
  leadLabel,
  relativeLabel,
  shortDate,
  timeLabel,
  upcomingRow,
  validateForm,
  type CalendarEvent,
  type EventInstance,
} from "./calendar.ts";

const event = (overrides: Partial<CalendarEvent>): CalendarEvent => ({
  id: "e1",
  source: "local",
  title: "Standup",
  notes: "",
  start: "2026-10-23T14:30",
  allDay: false,
  durationMinutes: null,
  recurrence: "none",
  remindBeforeMinutes: null,
  createdMs: 0,
  updatedMs: 0,
  ...overrides,
});

test("the pill says 'in N min' until the start, then 'now'", () => {
  assert.equal(relativeLabel(10), "in 10 min");
  assert.equal(relativeLabel(1), "in 1 min");
  assert.equal(relativeLabel(0), "now");
  assert.equal(relativeLabel(-3), "now");
});

test("the day is Today, Tomorrow, or the short date, across month ends", () => {
  assert.equal(dayLabel("2026-10-23T09:00", "2026-10-23"), "Today");
  assert.equal(dayLabel("2026-10-24T09:00", "2026-10-23"), "Tomorrow");
  assert.equal(dayLabel("2026-11-01T09:00", "2026-10-31"), "Tomorrow");
  assert.equal(dayLabel("2026-10-26T09:00", "2026-10-23"), "Mon 26 Oct");
  assert.equal(dayLabel("2026-10-22T09:00", "2026-10-23"), "Thu 22 Oct");
});

test("short dates name the weekday, day and month", () => {
  assert.equal(shortDate("2026-10-23"), "Fri 23 Oct");
  assert.equal(shortDate("2028-02-29"), "Tue 29 Feb");
  assert.equal(shortDate("not a date"), "");
});

test("an all-day event reads All day, and a timed one reads its time", () => {
  assert.equal(timeLabel("2026-10-23T00:00", true), "All day");
  assert.equal(timeLabel("2026-10-23T14:30", false), "14:30");
});

test("the settings line says the day, the time and any repeat", () => {
  assert.equal(describeEvent(event({})), "Fri 23 Oct · 14:30");
  assert.equal(
    describeEvent(event({ recurrence: "weekly" })),
    "Fri 23 Oct · 14:30 · Weekly",
  );
  assert.equal(
    describeEvent(event({ allDay: true, start: "2026-10-23T00:00", recurrence: "yearly" })),
    "Fri 23 Oct · All day · Yearly",
  );
});

test("the lead and hour choices read as words", () => {
  assert.equal(leadLabel(5), "5 minutes");
  assert.equal(leadLabel(30), "30 minutes");
  assert.equal(leadLabel(60), "1 hour");
  assert.equal(hourLabel(9), "09:00");
  assert.equal(hourLabel(23), "23:00");
});

test("dates must exist, including 29 February only in leap years", () => {
  assert.equal(isValidDate("2026-10-23"), true);
  assert.equal(isValidDate("2028-02-29"), true);
  assert.equal(isValidDate("2027-02-29"), false);
  assert.equal(isValidDate("2026-02-30"), false);
  assert.equal(isValidDate("2026-13-01"), false);
  assert.equal(isValidDate("23/10/2026"), false);
  assert.equal(isValidDate(""), false);
});

test("times are HH:MM on a 24-hour clock", () => {
  assert.equal(isValidTime("00:00"), true);
  assert.equal(isValidTime("23:59"), true);
  assert.equal(isValidTime("24:00"), false);
  assert.equal(isValidTime("9:30"), false);
  assert.equal(isValidTime(""), false);
});

test("a blank title and a bad date or time are reported inline", () => {
  const blank = { ...emptyForm("2026-10-23", "09:00"), title: "   " };
  assert.equal(validateForm(blank).title, "Give the event a title.");
  assert.equal(hasErrors(validateForm(blank)), true);

  const badDate = { ...emptyForm("2026-02-30", "09:00"), title: "Lunch" };
  assert.equal(validateForm(badDate).date, "Pick a date.");

  const badTime = { ...emptyForm("2026-10-23", "25:00"), title: "Lunch" };
  assert.equal(validateForm(badTime).time, "Pick a time.");
});

test("an all-day event does not need a time, and a valid form has no errors", () => {
  const allDay = { ...emptyForm("2026-10-23", ""), title: "Holiday", allDay: true };
  assert.deepEqual(validateForm(allDay), {});
  assert.equal(formStart(allDay), "2026-10-23T00:00");

  const timed = { ...emptyForm("2026-10-23", "14:30"), title: "Lunch" };
  assert.equal(hasErrors(validateForm(timed)), false);
  assert.equal(formStart(timed), "2026-10-23T14:30");
});

test("today's key is the local date, two digits a part", () => {
  assert.equal(dateKey(new Date(2026, 0, 5, 12, 0)), "2026-01-05");
  assert.equal(dateKey(new Date(2026, 9, 23, 23, 59)), "2026-10-23");
});

test("the island row reads the day, the time and the title of an occurrence", () => {
  const instance: EventInstance = {
    eventId: "e1",
    source: "local",
    title: "Standup",
    notes: "",
    start: "2026-10-23T14:30",
    end: "2026-10-23T14:30",
    allDay: false,
    readOnly: false,
    remindBeforeMinutes: null,
  };
  assert.deepEqual(upcomingRow(instance, "2026-10-23"), {
    day: "Today",
    time: "14:30",
    title: "Standup",
  });
  assert.deepEqual(upcomingRow({ ...instance, allDay: true }, "2026-10-22"), {
    day: "Tomorrow",
    time: "All day",
    title: "Standup",
  });
});
