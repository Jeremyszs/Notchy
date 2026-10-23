import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_NOTICE,
  type CalendarEvent,
  type CalendarNotice,
  type EventInstance,
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
