// The calendar's view rules. Pure: the labels, the date and time formats and the form checks are
// tested without a window. The types mirror CalendarEvent, EventInstance and the reminder notice in
// src-tauri/src/modules/calendar.rs. Times are local, as "YYYY-MM-DDTHH:MM", the way the backend
// writes them.

export type Recurrence = "none" | "daily" | "weekly" | "monthly" | "yearly";

/** One stored event, which may repeat. */
export interface CalendarEvent {
  id: string;
  source: string;
  title: string;
  notes: string;
  start: string;
  allDay: boolean;
  durationMinutes: number | null;
  recurrence: Recurrence;
  remindBeforeMinutes: number[] | null;
  createdMs: number;
  updatedMs: number;
}

/** One occurrence of an event, as the island and the reminders see it. */
export interface EventInstance {
  eventId: string;
  source: string;
  title: string;
  notes: string;
  start: string;
  end: string;
  allDay: boolean;
  readOnly: boolean;
  remindBeforeMinutes: number[] | null;
}

/** A reminder on screen. `minutesUntil` is zero once the start has passed. */
export interface CalendarNotice {
  seq: number;
  title: string;
  minutesUntil: number;
}

export const EMPTY_NOTICE: CalendarNotice = { seq: 0, title: "", minutesUntil: 0 };

/** The reminder a snooze can still reach. Mirrors RecentReminder in src-tauri/src/modules/calendar. */
export interface RecentReminder {
  title: string;
  /** Milliseconds left before the snooze window closes. */
  expiresInMs: number;
}

/** The form in Settings. `date` and `time` are the native input values. */
export interface EventForm {
  title: string;
  notes: string;
  date: string;
  time: string;
  allDay: boolean;
  recurrence: Recurrence;
  /** Minutes the event lasts, or null for no length. Timed events only. */
  durationMinutes: number | null;
  /** Minutes before the start to remind, or null for the Settings lead time. An empty list is the start only. */
  remindBeforeMinutes: number[] | null;
}

export interface FormErrors {
  title?: string;
  date?: string;
  time?: string;
  duration?: string;
}

/** What Settings sends to calendar_upsert. Mirrors EventInput in src-tauri/src/modules/calendar/model.rs. */
export interface EventInput {
  id: string | null;
  title: string;
  notes: string;
  start: string;
  allDay: boolean;
  recurrence: Recurrence;
  durationMinutes: number | null;
  remindBeforeMinutes: number[] | null;
}

/** The lead times the Settings window offers, in minutes. Mirrors LEAD_CHOICES in config.rs. */
export const LEAD_OPTIONS = [5, 10, 15, 30, 60] as const;

/** The snooze lengths the Calendar page offers, in minutes. Mirrors the backend's accepted values. */
export const SNOOZE_OPTIONS = [5, 10] as const;

/** The lengths the form offers, in minutes. "Custom" takes any whole number up to the maximum. */
export const LENGTH_OPTIONS = [15, 30, 45, 60, 90, 120] as const;

/** The longest length an event can have, one day. Mirrors the backend's check. */
export const MAX_DURATION_MINUTES = 1440;

/** The per-event reminders the form offers, in minutes. */
export const REMIND_OPTIONS = [5, 10, 15, 30, 60, 1440] as const;

/** The most reminders one event can have. Mirrors the backend's check. */
export const REMIND_MAX = 5;

const REPEAT_WORDS: Record<Recurrence, string> = {
  none: "",
  daily: "Daily",
  weekly: "Weekly",
  monthly: "Monthly",
  yearly: "Yearly",
};

export const REPEAT_OPTIONS: { value: Recurrence; label: string }[] = [
  { value: "none", label: "Does not repeat" },
  { value: "daily", label: "Daily" },
  { value: "weekly", label: "Weekly" },
  { value: "monthly", label: "Monthly" },
  { value: "yearly", label: "Yearly" },
];

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DATE_RE = /^(\d{4})-(\d{2})-(\d{2})$/;
const TIME_RE = /^([01]\d|2[0-3]):([0-5]\d)$/;
const DAY_MS = 86_400_000;

/**
 * The pill's words for a reminder. "now" once the start has passed, otherwise "in N min". Shared
 * by the pill and the tests, so the wording is in one place.
 */
export function relativeLabel(minutesUntil: number): string {
  return minutesUntil <= 0 ? "now" : `in ${minutesUntil} min`;
}

/** The date part of "YYYY-MM-DDTHH:MM". */
export function datePart(start: string): string {
  return start.slice(0, 10);
}

/** The time part of "YYYY-MM-DDTHH:MM", as HH:MM. */
export function timePart(start: string): string {
  return start.slice(11, 16);
}

/** Today's date on this PC, as YYYY-MM-DD. */
export function dateKey(now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

/** The day count since the epoch for a YYYY-MM-DD date. Equal days give equal counts. */
function dayNumber(date: string): number {
  const match = DATE_RE.exec(date);
  if (!match) return Number.NaN;
  return Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])) / DAY_MS;
}

/** Whether `date` is a real calendar date in YYYY-MM-DD. 2027-02-29 is not. */
export function isValidDate(date: string): boolean {
  const match = DATE_RE.exec(date);
  if (!match) return false;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const check = new Date(Date.UTC(year, month - 1, day));
  return (
    check.getUTCFullYear() === year &&
    check.getUTCMonth() === month - 1 &&
    check.getUTCDate() === day
  );
}

/** Whether `time` is HH:MM on a 24-hour clock. */
export function isValidTime(time: string): boolean {
  return TIME_RE.test(time);
}

/** "Mon 26 Oct", for a YYYY-MM-DD date. No year, since the list is always near. */
export function shortDate(date: string): string {
  const match = DATE_RE.exec(date);
  if (!match) return "";
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const weekday = WEEKDAYS[new Date(Date.UTC(year, month - 1, day)).getUTCDay()];
  return `${weekday} ${day} ${MONTHS[month - 1]}`;
}

/** "Today", "Tomorrow", or the short date, for an event's start against today's date. */
export function dayLabel(start: string, today: string): string {
  const days = dayNumber(datePart(start)) - dayNumber(today);
  if (days === 0) return "Today";
  if (days === 1) return "Tomorrow";
  return shortDate(datePart(start));
}

/** The time as HH:MM, or "All day" for an all-day event. */
export function timeLabel(start: string, allDay: boolean): string {
  return allDay ? "All day" : timePart(start);
}

/** The line under an event's title in Settings: its day, its time and how it repeats. */
export function describeEvent(event: CalendarEvent): string {
  const parts = [shortDate(datePart(event.start)), timeLabel(event.start, event.allDay)];
  const repeat = REPEAT_WORDS[event.recurrence];
  if (repeat) parts.push(repeat);
  return parts.join(" · ");
}

/** The lead time as a choice label, such as "10 minutes" or "1 hour". */
export function leadLabel(minutes: number): string {
  if (minutes === 60) return "1 hour";
  return `${minutes} minutes`;
}

/** An hour of the day as HH:00, for the all-day choice. */
export function hourLabel(hour: number): string {
  return `${String(hour).padStart(2, "0")}:00`;
}

/** A blank form for a new event, on `date` at `time`. It has no length and the Settings lead time. */
export function emptyForm(date: string, time: string): EventForm {
  return {
    title: "",
    notes: "",
    date,
    time,
    allDay: false,
    recurrence: "none",
    durationMinutes: null,
    remindBeforeMinutes: null,
  };
}

/** The form that edits an existing event. */
export function formFromEvent(event: CalendarEvent): EventForm {
  return {
    title: event.title,
    notes: event.notes,
    date: datePart(event.start),
    time: timePart(event.start),
    allDay: event.allDay,
    recurrence: event.recurrence,
    durationMinutes: event.durationMinutes,
    remindBeforeMinutes: event.remindBeforeMinutes,
  };
}

/** Whether `minutes` is a whole length from 1 to MAX_DURATION_MINUTES. */
export function isValidDuration(minutes: number): boolean {
  return Number.isInteger(minutes) && minutes >= 1 && minutes <= MAX_DURATION_MINUTES;
}

/** The length the form's choice stands for. "none" is no length, a number is that many minutes. */
export function durationFrom(choice: string, custom: string): number | null {
  if (choice === "none") return null;
  return Number(choice === "custom" ? custom : choice);
}

/** The form's length choice for a stored length: "none", the minutes as text, or "custom". */
export function lengthChoiceOf(minutes: number | null): string {
  if (minutes === null) return "none";
  return (LENGTH_OPTIONS as readonly number[]).includes(minutes) ? String(minutes) : "custom";
}

/** Adds or removes one reminder, keeping the list sorted and at most REMIND_MAX long. */
export function toggleReminder(current: number[] | null, minutes: number): number[] {
  const list = current ?? [];
  if (list.includes(minutes)) return list.filter((lead) => lead !== minutes);
  if (list.length >= REMIND_MAX) return list;
  return [...list, minutes].sort((a, b) => a - b);
}

/** A reminder choice as a short chip label, such as "5 min", "1 hour" or "1 day". */
export function reminderChipLabel(minutes: number): string {
  if (minutes === 60) return "1 hour";
  if (minutes === 1440) return "1 day";
  return `${minutes} min`;
}

/** The checks the form shows inline. The backend checks the same things again. */
export function validateForm(form: EventForm): FormErrors {
  const errors: FormErrors = {};
  if (form.title.trim() === "") errors.title = "Give the event a title.";
  if (!isValidDate(form.date)) errors.date = "Pick a date.";
  if (!form.allDay && !isValidTime(form.time)) errors.time = "Pick a time.";
  if (!form.allDay && form.durationMinutes !== null && !isValidDuration(form.durationMinutes)) {
    errors.duration = `Length must be 1 to ${MAX_DURATION_MINUTES} minutes.`;
  }
  return errors;
}

/** Whether any check failed. */
export function hasErrors(errors: FormErrors): boolean {
  return Object.keys(errors).length > 0;
}

/** The start the backend expects. An all-day event is held to midnight. */
export function formStart(form: EventForm): string {
  return `${form.date}T${form.allDay ? "00:00" : form.time}`;
}

/**
 * What the backend is sent for a form. An all-day event has one reminder and no length, so both
 * fields are null for it, as the backend expects.
 */
export function eventInput(form: EventForm, id: string | null): EventInput {
  return {
    id,
    title: form.title.trim(),
    notes: form.notes,
    start: formStart(form),
    allDay: form.allDay,
    recurrence: form.recurrence,
    durationMinutes: form.allDay ? null : form.durationMinutes,
    remindBeforeMinutes: form.allDay ? null : form.remindBeforeMinutes,
  };
}

/** One row of the island's Calendar page. */
export interface UpcomingRow {
  day: string;
  time: string;
  title: string;
}

/** The row the Calendar page shows for an occurrence, read against today's date. */
export function upcomingRow(instance: EventInstance, today: string): UpcomingRow {
  return {
    day: dayLabel(instance.start, today),
    time: timeLabel(instance.start, instance.allDay),
    title: instance.title,
  };
}
