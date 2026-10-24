// The notification pill's and settings' view rules. Pure, so the copy and the list rules are
// tested without a window. The types mirror Notice and Snapshot in
// src-tauri/src/modules/notifications.rs. The text is already trimmed and cut there.

/** One notification on screen. `seq` is new for each notification. */
export interface Notice {
  seq: number;
  app: string;
  title: string;
  body: string;
  /** The app's logo as a data URI, when Windows gave one. */
  icon?: string;
}

/** What Windows allows. `unknown` until the listener has been read. */
export type NotificationAccess = "unknown" | "allowed" | "denied" | "unspecified";

/** An app that has sent a notification. `id` is its AppUserModelId, the key `deniedApps` uses. */
export interface SeenApp {
  id: string;
  name: string;
  /** The app's logo as a data URI, read the first time the app is seen. */
  icon?: string;
}

/** The settings view. Mirrors the notifications snapshot. */
export interface NotificationsSnapshot {
  access: NotificationAccess;
  apps: SeenApp[];
}

export const EMPTY_NOTICE: Notice = { seq: 0, app: "", title: "", body: "" };
export const EMPTY_SNAPSHOT: NotificationsSnapshot = { access: "unknown", apps: [] };

/** The letter in the pill's badge: the app name's first character, upper-cased. */
export function initialOf(app: string): string {
  const first = Array.from(app.trim())[0];
  return first ? first.toUpperCase() : "•";
}

/** The line under the access switch. Says what Windows setting to change, with no button. */
export function accessCopy(access: NotificationAccess): string {
  switch (access) {
    case "allowed":
      return "Windows lets Notchy read notifications.";
    case "denied":
      return "Windows is blocking Notchy from reading notifications. Turn on Let apps access your notifications in Settings > Privacy & security > Notifications.";
    case "unspecified":
      return "Windows has not decided yet. Turn on Let apps access your notifications in Settings > Privacy & security > Notifications.";
    default:
      return "Checking access to notifications.";
  }
}

/** Whether an app's notifications are shown. Ids match without case, as Windows compares them. */
export function isAllowed(id: string, denied: readonly string[]): boolean {
  const key = id.toLowerCase();
  return !denied.some((entry) => entry.toLowerCase() === key);
}

/** The denied list after an app is allowed or denied. Returns a new array, and no repeats. */
export function setAllowed(denied: readonly string[], id: string, allowed: boolean): string[] {
  const rest = denied.filter((entry) => entry.toLowerCase() !== id.toLowerCase());
  return allowed ? rest : [...rest, id];
}
