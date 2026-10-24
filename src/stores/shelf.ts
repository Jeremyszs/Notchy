import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShelfItem, ShelfMode } from "../lib/shelf";
import { showShelfDrop } from "./pages";

const store = writable<ShelfItem[]>([]);
export const shelf = { subscribe: store.subscribe };
export const shelfDrag = writable(false);
// The mode the user picked, kept while the app runs. Null until a pick, so the page uses its default.
export const shelfMode = writable<ShelfMode | null>(null);
export const shelfConfirmation = writable("");
let revision = 0;
let confirmationTimer: ReturnType<typeof setTimeout>;

// Attach before fetching, and discard a snapshot overtaken by an update.
void listen<ShelfItem[]>("shelf://update", ({ payload }) => {
  revision += 1;
  store.set(payload);
}).then(refreshShelf);

// Rust sees the drag (the catcher window takes the OLE drop), so the page switch lives here.
void listen<boolean>("shelf://drag", ({ payload }) => {
  shelfDrag.set(payload);
  // A file drag always lands on the Files mode.
  if (payload) {
    shelfMode.set("file");
    showShelfDrop();
  }
});

// Rust adds the dropped files and reports how many, for the header line.
void listen<number>("shelf://added", ({ payload }) => confirm(payload));

function confirm(added: number) {
  clearTimeout(confirmationTimer);
  shelfConfirmation.set(`${added} added`);
  confirmationTimer = setTimeout(() => shelfConfirmation.set(""), 1200);
}

export async function refreshShelf() {
  const before = revision;
  const items = await invoke<ShelfItem[]>("shelf_list").catch(() => null);
  if (items && revision === before) store.set(items);
}

export function clearShelfDrag() {
  shelfDrag.set(false);
  void invoke("shelf_drag_active", { active: false }).catch(() => {});
}
