import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ShelfItem } from "../lib/shelf";

const store = writable<ShelfItem[]>([]);
export const shelf = { subscribe: store.subscribe };
export const shelfDrag = writable(false);
export const shelfConfirmation = writable("");
let revision = 0;
let confirmationTimer: ReturnType<typeof setTimeout>;

// Attach before fetching, and discard a snapshot overtaken by an update.
void listen<ShelfItem[]>("shelf://update", ({ payload }) => {
  revision += 1;
  store.set(payload);
}).then(refreshShelf);
void listen<boolean>("shelf://drag", ({ payload }) => shelfDrag.set(payload));

export async function refreshShelf() {
  const before = revision;
  const items = await invoke<ShelfItem[]>("shelf_list").catch(() => null);
  if (items && revision === before) store.set(items);
}

export async function addFiles(paths: string[]) {
  const added = await invoke<number>("shelf_add_files", { paths });
  clearTimeout(confirmationTimer);
  shelfConfirmation.set(`${added} added`);
  confirmationTimer = setTimeout(() => shelfConfirmation.set(""), 1200);
}

export function clearShelfDrag() {
  shelfDrag.set(false);
  void invoke("shelf_drag_active", { active: false }).catch(() => {});
}
