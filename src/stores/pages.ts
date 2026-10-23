import { derived, get, writable } from "svelte/store";
import { mediaState } from "./media";
import { config } from "./config";

export type Page = "media" | "controls" | "timer";

export const PAGE_LABELS: Record<Page, string> = {
  media: "Media",
  controls: "Controls",
  timer: "Timer",
};

// A page the user left this recently is the one they get back when the panel reopens.
const RESUME_WINDOW_MS = 8000;

/**
 * The page to show when the panel opens. Pure: the caller passes the page left last, how long
 * ago it was left (Infinity when never), and whether media is playing.
 */
export function choosePage(
  lastPage: Page | null,
  msSinceLeft: number,
  mediaPlaying: boolean,
): Page {
  if (lastPage !== null && msSinceLeft < RESUME_WINDOW_MS) return lastPage;
  return mediaPlaying ? "media" : "controls";
}

function isPage(id: string): id is Page {
  return id in PAGE_LABELS;
}

// Pages the pager cycles through, in order, from the settings. With one page enabled, the pager
// is hidden.
export const enabledPages = derived(config, ($config) =>
  $config.pages.filter(isPage),
);

export const page = writable<Page>("media");

let lastPage: Page | null = null;
let leftAt: number | null = null;

/** Called when the panel opens. Picks the page for this opening. */
export function enterPanel(): void {
  const enabled = get(enabledPages);
  const msSinceLeft = leftAt === null ? Infinity : Date.now() - leftAt;
  const chosen = choosePage(lastPage, msSinceLeft, get(mediaState).playing);
  page.set(enabled.includes(chosen) ? chosen : (enabled[0] ?? "media"));
}

/** Called when the panel closes. Remembers the page so a quick reopen keeps it. */
export function leavePanel(): void {
  lastPage = get(page);
  leftAt = Date.now();
}

/** Shows `target` if it is enabled. */
export function go(target: Page): void {
  if (get(enabledPages).includes(target)) page.set(target);
}

/** Moves one page forward (1) or back (-1). Stops at the ends rather than wrapping. */
export function step(direction: 1 | -1): void {
  const enabled = get(enabledPages);
  const index = enabled.indexOf(get(page));
  const target = Math.min(Math.max(index + direction, 0), enabled.length - 1);
  const next = enabled[target];
  if (next !== undefined) go(next);
}
