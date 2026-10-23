import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  EMPTY_NOTICE,
  EMPTY_SNAPSHOT,
  type Notice,
  type NotificationsSnapshot,
} from "../lib/notifications";

// The Rust side owns the queue and decides when the notification state starts and ends (see
// islandState). Each notice arrives on notifications://show before the state changes. The last
// one is kept after it hides, so the pill can shrink away with its own text.
export const notice = readable<Notice>(EMPTY_NOTICE, (set) => {
  const unlisten = listen<Notice>("notifications://show", (event) => {
    set(event.payload);
  });

  return () => {
    unlisten.then((stop) => stop());
  };
});

// Access and the apps seen so far, for the settings window. The snapshot is read on first
// subscribe, and every change after that arrives on notifications://update.
export const notificationsStatus = readable<NotificationsSnapshot>(EMPTY_SNAPSHOT, (set) => {
  let heard = false;
  const unlisten = listen<NotificationsSnapshot>("notifications://update", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<NotificationsSnapshot>("notifications_snapshot").then(
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
