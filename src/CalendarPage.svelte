<script lang="ts">
  // The island's Calendar page: the month on the left, the selected day's notes on the right. The
  // backend expands repeating events (calendar_between), so this file only lays them out. The island
  // window cannot take text, so "Add note" opens Settings on the selected day, where the form is.
  import { invoke } from "@tauri-apps/api/core";
  import { cubicOut } from "svelte/easing";
  import { fade, fly } from "svelte/transition";
  import { calendarEvents, fetchBetween } from "./stores/calendar";
  import { islandState } from "./stores/island";
  import { dateKey, shortDate, timeLabel, type EventInstance } from "./lib/calendar";
  import {
    cellLabel,
    firstWeekday,
    gridRange,
    instancesOn,
    monthGrid,
    monthOf,
    monthTitle,
    noteCounts,
    noteRows,
    shiftKeyByMonth,
    weekdayHeaders,
  } from "./lib/month";

  // Read once, the way the island reads it. A change while the app runs needs a restart.
  const REDUCED = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const FIRST_DAY = firstWeekday(typeof navigator === "undefined" ? "" : navigator.language);
  const HEADERS = weekdayHeaders(FIRST_DAY);

  let today = dateKey(new Date());
  let selected = today;
  // -1 for a move back in time, 1 for forward. The month's slide follows it.
  let direction = 0;
  let instances: EventInstance[] = [];
  let request = 0;
  let wasOpen = false;

  $: view = monthOf(selected);
  $: monthIndex = view.year * 12 + view.month;
  $: rows = monthGrid(view.year, view.month, FIRST_DAY);
  $: range = gridRange(rows);
  $: counts = noteCounts(instances);
  $: dayNotes = instancesOn(instances, selected);
  $: shown = noteRows(dayNotes.length);
  // Reloads when the month changes, and when the event list changes. The old notes stay on screen until
  // the new ones arrive, so a refresh does not blank the grid.
  $: reload(range.from, range.to, $calendarEvents);
  $: onPanel($islandState === "expanded");

  // Opening the panel shows today, and reads the date again, since the panel can stay shut overnight.
  function onPanel(open: boolean): void {
    if (open && !wasOpen) {
      today = dateKey(new Date());
      direction = 0;
      selected = today;
    }
    wasOpen = open;
  }

  async function reload(from: string, to: string, _list: unknown): Promise<void> {
    const id = ++request;
    try {
      const found = await fetchBetween(from, to);
      if (id === request) instances = found;
    } catch {
      // A failed read must not leave the last month's notes under this one.
      if (id === request) instances = [];
    }
  }

  function indexOf(key: string): number {
    const { year, month } = monthOf(key);
    return year * 12 + month;
  }

  function select(key: string): void {
    direction = Math.sign(indexOf(key) - indexOf(selected));
    selected = key;
  }

  // Month change is by the arrows only. The wheel turns pages, so it never changes the month.
  function moveMonth(delta: number): void {
    select(shiftKeyByMonth(selected, delta));
  }

  function addNote(): void {
    invoke("settings_open_calendar", { date: selected }).catch((error) =>
      console.warn("Could not open Settings on the calendar", error),
    );
  }

  const motionIn = (node: Element) =>
    REDUCED
      ? fade(node, { duration: 150 })
      : fly(node, { x: direction * 16, duration: 220, easing: cubicOut });
  const motionOut = (node: Element) => fade(node, { duration: REDUCED ? 150 : 120 });
</script>

<div class="calendar" role="group" aria-label="Calendar">
  <div class="nav">
    <button type="button" class="arrow" aria-label="Previous month" onclick={() => moveMonth(-1)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" /></svg>
    </button>
    <span class="month-title" aria-live="polite">{monthTitle(view.year, view.month)}</span>
    <button type="button" class="arrow" aria-label="Next month" onclick={() => moveMonth(1)}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3l5 5-5 5" /></svg>
    </button>
  </div>

  <div class="weekdays" aria-hidden="true">
    {#each HEADERS as header, column (column)}
      <span>{header}</span>
    {/each}
  </div>

  <div class="month-grid">
    {#key monthIndex}
      <div class="sheet" in:motionIn|local out:motionOut|local>
        {#each rows as week (week[0].key)}
          <div class="week">
            {#each week as cell (cell.key)}
              {@const count = counts.get(cell.key) ?? 0}
              <button
                type="button"
                class="day"
                class:has={count > 0}
                class:other={!cell.inMonth}
                class:today={cell.key === today}
                class:selected={cell.key === selected}
                aria-label={cellLabel(cell.key, count)}
                aria-pressed={cell.key === selected}
                onclick={() => select(cell.key)}
              >
                <span class="num">{cell.day}</span>
              </button>
            {/each}
          </div>
        {/each}
      </div>
    {/key}
  </div>

  <div class="divider" aria-hidden="true"></div>

  <div class="side-head">
    <span class="when">{shortDate(selected)}</span>
    <button type="button" class="add" onclick={addNote}>Add note</button>
  </div>

  <div class="notes">
    {#if dayNotes.length === 0}
      <p class="empty">No notes</p>
    {:else}
      <ul class="list">
        {#each dayNotes.slice(0, shown.shown) as note (`${note.eventId}|${note.start}`)}
          <li class="note">
            <span class="time">{timeLabel(note.start, note.allDay)}</span>
            <span class="name">{note.title}</span>
          </li>
        {/each}
        {#if shown.more > 0}
          <li class="more">+{shown.more} more</li>
        {/if}
      </ul>
    {/if}
  </div>
</div>

<style>
  /* Two columns and a divider, in one grid so both sides share the same rows. Widths: the month is
     7 x 26 px, then 12 px, a 1 px divider, 12 px, then the notes (348 px in all). Heights: a 18 px
     heading, a 4 px gap, a 12 px weekday row, a 4 px gap and six 17 px rows, 140 px in all. The page
     centres the 140 px block in its 146 px box, so the space above and below is 3 px each. */
  .calendar {
    display: grid;
    grid-template-columns: 182px 25px 141px;
    grid-template-rows: 18px 4px 12px 4px 102px;
    width: 100%;
    color: #fff;
  }

  .nav {
    grid-column: 1;
    grid-row: 1;
    display: grid;
    grid-template-columns: 24px minmax(0, 1fr) 24px;
    align-items: center;
    height: 18px;
  }

  /* The arrows are equal, so the title centres between them. */
  .arrow {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 9px;
    background: none;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .arrow:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  .arrow svg {
    width: 12px;
    height: 12px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .month-title {
    font-size: 13px;
    font-weight: 600;
    line-height: 18px;
    text-align: center;
    white-space: nowrap;
  }

  .weekdays {
    grid-column: 1;
    grid-row: 3;
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    align-items: center;
    height: 12px;
    font-size: 11px;
    font-weight: 600;
    line-height: 12px;
    text-align: center;
    color: rgba(255, 255, 255, 0.55);
  }

  /* Six rows, always, so the page never changes height between months. */
  .month-grid {
    position: relative;
    grid-column: 1;
    grid-row: 5;
    height: 102px;
  }

  .sheet {
    position: absolute;
    inset: 0;
  }

  .week {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    height: 17px;
  }

  /* The whole 26 x 17 cell is the hit area. The number is a 16 px disc inside it. */
  .day {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 100%;
    height: 17px;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }

  .num {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 16px;
    height: 16px;
    border-radius: 8px;
    font-size: 11px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    color: rgba(255, 255, 255, 0.9);
  }

  .day:not(.selected):not(.has):hover .num {
    background: rgba(255, 255, 255, 0.08);
  }

  /* A day with notes: the accent at 22% with accent text. */
  .day.has .num {
    background: rgba(255, 106, 61, 0.22);
    color: #ff6a3d;
    font-weight: 600;
  }

  .day.other .num {
    color: rgba(255, 255, 255, 0.4);
  }

  .day.today .num {
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.55);
  }

  /* The selected day is solid, with a black number (7.4:1 on the accent). */
  .day.selected .num {
    background: #ff6a3d;
    color: #000;
    font-weight: 600;
  }

  .divider {
    grid-column: 2;
    grid-row: 1 / 6;
    justify-self: center;
    width: 1px;
    background: rgba(255, 255, 255, 0.08);
  }

  .side-head {
    grid-column: 3;
    grid-row: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
    height: 18px;
  }

  .when {
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
    font-weight: 600;
    line-height: 18px;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.9);
  }

  .add {
    flex: none;
    height: 18px;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    line-height: 18px;
    color: #ff6a3d;
    cursor: pointer;
  }

  /* The notes body is the six grid rows' height, and its content is centred in it. */
  .notes {
    grid-column: 3;
    grid-row: 5;
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
    height: 102px;
  }

  .empty {
    margin: 0;
    font-size: 13px;
    line-height: 16px;
    text-align: center;
    color: rgba(255, 255, 255, 0.55);
  }

  .list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* Four 24 px rows make 96 px, so a full list is 3 px above and below. */
  .note,
  .more {
    display: grid;
    align-items: center;
    height: 24px;
  }

  .note {
    grid-template-columns: 40px minmax(0, 1fr);
    column-gap: 6px;
  }

  .time,
  .name,
  .more {
    white-space: nowrap;
  }

  .time {
    overflow: hidden;
    font-size: 11px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.6);
  }

  .name {
    min-width: 0;
    overflow: hidden;
    font-size: 12px;
    font-weight: 500;
    line-height: 24px;
    text-align: left;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.9);
  }

  .more {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.55);
  }

  button:focus-visible {
    outline: 1px solid rgba(255, 255, 255, 0.6);
    outline-offset: 1px;
  }
</style>
