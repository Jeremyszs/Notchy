// The idle pill's clock. Pure, so the minute arithmetic and the format are tested without a window.

/** Milliseconds from `now` to the next whole minute. Always from 1 to 60000, so it never returns 0. */
export function msUntilNextMinute(now: Date): number {
  return 60_000 - (now.getSeconds() * 1000 + now.getMilliseconds());
}

/** The time as HH:MM on a 24-hour clock. */
export function formatHM(now: Date): string {
  const pad = (n: number) => (n < 10 ? `0${n}` : `${n}`);
  return `${pad(now.getHours())}:${pad(now.getMinutes())}`;
}
