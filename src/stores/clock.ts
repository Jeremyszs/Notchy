import { readable } from "svelte/store";
import { msUntilNextMinute } from "../lib/clock";

// The wall clock, to the minute. Each minute arms one timeout for the next boundary, so nothing
// polls. It runs only while something subscribes, so with the idle clock off no timer exists.
export const minuteClock = readable(new Date(), (set) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const tick = () => {
    const now = new Date();
    set(now);
    timer = setTimeout(tick, msUntilNextMinute(now));
  };
  tick();
  return () => clearTimeout(timer);
});
