import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_NOTICE,
  type CalendarEvent,
  type CalendarNotice,
  type EventInstance,
  type RecentReminder,
} from "../lib/calendar";

// The Rust side owns the reminder queue and decides when the calendar state starts and ends (see
// islandState). Each reminder arrives on calendar://show before the state changes. The last one is
// kept after it hides, so the pill can shrink away with its own text.
export const reminder = readable<CalendarNotice>(EMPTY_NOTICE, (set) => {
  const unlisten = listen<CalendarNotice>("calendar://show", (event) => {
    set(event.payload);
  });

  return () => {
    unlisten.then((stop) => stop());
  };
});

// The occurrences from `from` to `to`, both "YYYY-MM-DD" and both included. The Settings month grid asks
// for the days it shows, and asks again whenever the event list changes.
export function fetchBetween(from: string, to: string): Promise<EventInstance[]> {
  return invoke<EventInstance[]>("calendar_between", { from, to });
}

// The whole event list, for the settings window. Read on first subscribe, and every change after
// that arrives on calendar://update.
export const calendarEvents = readable<CalendarEvent[]>([], (set) => {
  let heard = false;
  const unlisten = listen<CalendarEvent[]>("calendar://update", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<CalendarEvent[]>("calendar_list").then(
    (list) => {
      // An update that arrived first is newer than this list, so it is kept.
      if (!heard) set(list);
    },
    () => {},
  );

  return () => {
    unlisten.then((stop) => stop());
  };
});

// The reminder a snooze can still reach, or null. The Rust side sends it when a reminder fires, when it is
// snoozed (null again) and when its 30 minutes run out (null). It is read once on subscribe.
export const recentReminder = readable<RecentReminder | null>(null, (set) => {
  let heard = false;
  const unlisten = listen<RecentReminder | null>("calendar://recent", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<RecentReminder | null>("calendar_recent_reminder").then(
    (snapshot) => {
      // An update that arrived first is newer than this snapshot, so it is kept.
      if (!heard) set(snapshot);
    },
    () => {},
  );

  return () => {
    unlisten.then((stop) => stop());
  };
});

// Snoozes the recent reminder by 5 or 10 minutes. Resolves to the error text the person should read, or null.
export async function snooze(minutes: 5 | 10): Promise<string | null> {
  try {
    await invoke<void>("calendar_snooze", { minutes });
    return null;
  } catch (error) {
    return String(error);
  }
}
