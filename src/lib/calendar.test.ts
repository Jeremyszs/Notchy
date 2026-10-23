import { test } from "node:test";
import assert from "node:assert/strict";
import {
  dateKey,
  dayLabel,
  describeEvent,
  durationFrom,
  emptyForm,
  eventInput,
  formFromEvent,
  formStart,
  hasErrors,
  hourLabel,
  isValidDate,
  isValidDuration,
  isValidTime,
  leadLabel,
  lengthChoiceOf,
  reminderChipLabel,
  relativeLabel,
  shortDate,
  timeLabel,
  toggleReminder,
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

test("a length is a whole number of minutes from 1 to a day", () => {
  assert.equal(isValidDuration(1), true);
  assert.equal(isValidDuration(90), true);
  assert.equal(isValidDuration(1440), true);
  assert.equal(isValidDuration(0), false);
  assert.equal(isValidDuration(1441), false);
  assert.equal(isValidDuration(2.5), false);
  assert.equal(isValidDuration(Number.NaN), false);
});

test("the length choice reads back as the form's option", () => {
  assert.equal(lengthChoiceOf(null), "none");
  assert.equal(lengthChoiceOf(45), "45");
  assert.equal(lengthChoiceOf(50), "custom");
  assert.equal(durationFrom("none", ""), null);
  assert.equal(durationFrom("60", ""), 60);
  assert.equal(durationFrom("custom", "75"), 75);
  assert.equal(Number.isNaN(durationFrom("custom", "abc")), true);
});

test("a custom length outside the range is reported inline", () => {
  const timed = { ...emptyForm("2026-10-23", "14:30"), title: "Long call", durationMinutes: 0 };
  assert.equal(validateForm(timed).duration, "Length must be 1 to 1440 minutes.");
  assert.equal(validateForm({ ...timed, durationMinutes: 240 }).duration, undefined);
  // All-day events have no length, so a stale value is not checked.
  assert.deepEqual(validateForm({ ...timed, allDay: true }), {});
});

test("reminders toggle on and off, stay sorted, and stop at five", () => {
  assert.deepEqual(toggleReminder(null, 30), [30]);
  assert.deepEqual(toggleReminder([30], 5), [5, 30]);
  assert.deepEqual(toggleReminder([5, 30], 30), [5]);
  assert.deepEqual(toggleReminder([], 10), [10]);
  assert.deepEqual(toggleReminder([5, 10, 15, 30, 60], 1440), [5, 10, 15, 30, 60]);
  assert.deepEqual(toggleReminder([5, 10, 15, 30, 60], 5), [10, 15, 30, 60]);
});

test("reminder chips read as short words", () => {
  assert.equal(reminderChipLabel(5), "5 min");
  assert.equal(reminderChipLabel(60), "1 hour");
  assert.equal(reminderChipLabel(1440), "1 day");
});

test("the backend input carries the length and reminders of a timed event", () => {
  const form = {
    ...emptyForm("2026-10-23", "14:30"),
    title: "  Standup  ",
    durationMinutes: 30,
    remindBeforeMinutes: [5, 60],
  };
  assert.deepEqual(eventInput(form, null), {
    id: null,
    title: "Standup",
    notes: "",
    start: "2026-10-23T14:30",
    allDay: false,
    recurrence: "none",
    durationMinutes: 30,
    remindBeforeMinutes: [5, 60],
  });
  // An all-day event sends neither, whatever the form still holds.
  const allDay = eventInput({ ...form, allDay: true }, "e1");
  assert.equal(allDay.durationMinutes, null);
  assert.equal(allDay.remindBeforeMinutes, null);
  assert.equal(allDay.id, "e1");
});

test("editing an event starts the form from its length and reminders", () => {
  const stored = event({ durationMinutes: 45, remindBeforeMinutes: [] });
  const form = formFromEvent(stored);
  assert.equal(form.durationMinutes, 45);
  assert.deepEqual(form.remindBeforeMinutes, []);
  const fresh = emptyForm("2026-10-23", "09:00");
  assert.equal(fresh.durationMinutes, null);
  assert.equal(fresh.remindBeforeMinutes, null);
});
