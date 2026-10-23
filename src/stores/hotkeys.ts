import { readable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { HotkeyStatus } from "../lib/hotkeys";

// Which bindings took, as the Rust side registered them. It is sent on hotkeys://status after each
// change, and read once on subscribe, so Settings can show the reason a shortcut did not take.
export const hotkeysStatus = readable<HotkeyStatus[]>([], (set) => {
  let heard = false;
  const unlisten = listen<HotkeyStatus[]>("hotkeys://status", (event) => {
    heard = true;
    set(event.payload);
  });
  invoke<HotkeyStatus[]>("hotkeys_status").then(
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
