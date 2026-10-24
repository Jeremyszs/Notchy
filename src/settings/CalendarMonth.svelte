<script lang="ts">
  // The Calendar section's month view: a grid of the month, the days that have notes coloured, and the
  // notes of the selected day below it with add, edit and delete. The backend expands repeating events
  // (calendar_between), so this file only lays them out. It refreshes when the event list changes, which
  // arrives on calendar://update through the calendarEvents store.
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import { SMOOTH_MS, smoothEase } from "../lib/motion";
  import { fade, fly } from "svelte/transition";
  import { calendarEvents, fetchBetween } from "../stores/calendar";
  import {
    dateKey,
    formFromEvent,
    emptyForm,
    REPEAT_OPTIONS,
    timeLabel,
    type CalendarEvent,
    type EventForm,
    type EventInstance,
  } from "../lib/calendar.ts";
  import {
    cellLabel,
    defaultTime,
    firstWeekday,
    gridRange,
    instancesOn,
    longDate,
    monthGrid,
    monthOf,
    monthTitle,
    noteCounts,
    shiftKeyByMonth,
    stepKey,
    weekdayHeaders,
    type Step,
  } from "../lib/month.ts";
  import CalendarNoteForm from "./CalendarNoteForm.svelte";

  // Read once, the way the island reads it. A change while Settings is open needs a reopen.
  const REDUCED = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const FIRST_DAY = firstWeekday(typeof navigator === "undefined" ? "" : navigator.language);
  const HEADERS = weekdayHeaders(FIRST_DAY);
  const KEY_STEPS: Record<string, Step> = {
    ArrowLeft: "left",
    ArrowRight: "right",
    ArrowUp: "up",
    ArrowDown: "down",
    Home: "home",
    End: "end",
    PageUp: "pageUp",
    PageDown: "pageDown",
  };

  let today = dateKey(new Date());
  let selected = today;
  // -1 for a move back in time, 1 for forward. The month's slide follows it.
  let direction = 0;
  let instances: EventInstance[] = [];
  let loadError = "";
  let request = 0;
  let mode: "list" | "add" | "edit" = "list";
  let editingEvent: CalendarEvent | null = null;
  let formSeed: EventForm = emptyForm(today, defaultTime(new Date()));
  // Bumped on every open, so the form remounts and copies the new seed even while it is already shown.
  let formKey = 0;
  let confirmingKey: string | null = null;
  let listError = "";
  let gridEl: HTMLElement | undefined;
  let rootEl: HTMLElement | undefined;

  $: view = monthOf(selected);
  $: monthIndex = view.year * 12 + view.month;
  $: rows = monthGrid(view.year, view.month, FIRST_DAY);
  $: range = gridRange(rows);
  $: counts = noteCounts(instances);
  $: dayNotes = instancesOn(instances, selected);
  $: reload(range.from, range.to, $calendarEvents);

  // The newest request wins, so a slow answer never overwrites a newer month.
  async function reload(from: string, to: string, _list: CalendarEvent[]): Promise<void> {
    const id = ++request;
    try {
      const found = await fetchBetween(from, to);
      if (id === request) {
        instances = found;
        loadError = "";
      }
    } catch (error) {
      if (id === request) {
        loadError = String(error);
        instances = [];
      }
    }
  }

  function indexOf(key: string): number {
    const { year, month } = monthOf(key);
    return year * 12 + month;
  }

  function select(key: string): void {
    direction = Math.sign(indexOf(key) - indexOf(selected));
    selected = key;
    confirmingKey = null;
    listError = "";
  }

  function moveMonth(delta: number): void {
    select(shiftKeyByMonth(selected, delta));
  }

  async function onGridKey(event: KeyboardEvent): Promise<void> {
    const step = KEY_STEPS[event.key];
    if (!step) return;
    event.preventDefault();
    select(stepKey(selected, step, FIRST_DAY));
    await tick();
    // Look only in the sheet for this month, since the one that is leaving may hold the same day.
    gridEl
      ?.querySelector<HTMLButtonElement>(`.sheet[data-month="${monthIndex}"] [data-key="${selected}"]`)
      ?.focus();
  }

  function repeatLabel(note: EventInstance): string {
    const series = seriesOf(note);
    if (!series || series.recurrence === "none") return "";
    return REPEAT_OPTIONS.find((option) => option.value === series.recurrence)?.label ?? "";
  }

  function seriesOf(note: EventInstance): CalendarEvent | undefined {
    return $calendarEvents.find((event) => event.id === note.eventId);
  }

  function noteKey(note: EventInstance): string {
    return `${note.eventId}|${note.start}`;
  }

  function startAdd(): void {
    formSeed = emptyForm(selected, defaultTime(new Date()));
    formKey += 1;
    editingEvent = null;
    mode = "add";
  }

  function startEdit(note: EventInstance): void {
    const series = seriesOf(note);
    if (!series) {
      listError = "That note no longer exists.";
      return;
    }
    formSeed = formFromEvent(series);
    formKey += 1;
    editingEvent = series;
    mode = "edit";
  }

  function closeForm(): void {
    mode = "list";
    editingEvent = null;
    listError = "";
  }

  async function remove(note: EventInstance): Promise<void> {
    confirmingKey = null;
    listError = "";
    try {
      await invoke("calendar_delete", { id: note.eventId });
    } catch (error) {
      listError = String(error);
    }
  }

  const motionIn = (node: Element) =>
    REDUCED
      ? fade(node, { duration: 150 })
      : fly(node, { x: direction * 16, duration: SMOOTH_MS, easing: smoothEase });
  const motionOut = (node: Element) => fade(node, { duration: REDUCED ? 150 : 120 });

  // The island's "Add note" opens this section on its day with the add form. A window that is already
  // open hears the event. A window built for the request may not be listening yet, so it takes the day
  // once when it mounts.
  const DAY_KEY = /^\d{4}-\d{2}-\d{2}$/;

  function openAddFor(day: string): void {
    if (!DAY_KEY.test(day)) return;
    select(day);
    formSeed = emptyForm(day, defaultTime(new Date()));
    formKey += 1;
    editingEvent = null;
    listError = "";
    mode = "add";
    rootEl?.closest("section")?.scrollIntoView({ block: "start" });
  }

  onMount(() => {
    const refreshToday = () => {
      today = dateKey(new Date());
    };
    window.addEventListener("focus", refreshToday);
    let stop: (() => void) | undefined;
    let gone = false;
    listen<string>("calendar://add-note", (event) => openAddFor(event.payload)).then((off) => {
      if (gone) off();
      else stop = off;
    });
    invoke<string | null>("calendar_take_add_date").then(
      (day) => {
        if (day) openAddFor(day);
      },
      () => {},
    );
    return () => {
      window.removeEventListener("focus", refreshToday);
      gone = true;
      stop?.();
    };
  });
</script>

<div class="month" bind:this={rootEl}>
  <div class="card grid-card">
    <div class="bar">
      <div class="nav">
        <button type="button" class="arrow" aria-label="Previous month" onclick={() => moveMonth(-1)}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" /></svg>
        </button>
        <span class="title" aria-live="polite">{monthTitle(view.year, view.month)}</span>
        <button type="button" class="arrow" aria-label="Next month" onclick={() => moveMonth(1)}>
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M6 3l5 5-5 5" /></svg>
        </button>
      </div>
      <button type="button" class="today" onclick={() => select(today)}>Today</button>
    </div>

    <div class="weekdays" aria-hidden="true">
      {#each HEADERS as header (header)}
        <span>{header}</span>
      {/each}
    </div>

    <div
      class="frame"
      role="grid"
      tabindex="-1"
      aria-label={monthTitle(view.year, view.month)}
      bind:this={gridEl}
      onkeydown={onGridKey}
    >
      {#key monthIndex}
        <div class="sheet" role="rowgroup" data-month={monthIndex} in:motionIn|local out:motionOut|local>
          {#each rows as week (week[0].key)}
            <div class="week" role="row">
              {#each week as cell (cell.key)}
                {@const count = counts.get(cell.key) ?? 0}
                {@const isSelected = cell.key === selected}
                <div class="cell" role="gridcell" aria-selected={isSelected}>
                  <button
                    type="button"
                    class="day"
                    class:has={count > 0}
                    class:other={!cell.inMonth}
                    class:today={cell.key === today}
                    class:selected={isSelected}
                    data-key={cell.key}
                    tabindex={isSelected ? 0 : -1}
                    aria-label={cellLabel(cell.key, count)}
                    aria-current={cell.key === today ? "date" : undefined}
                    onclick={() => select(cell.key)}
                  >
                    {cell.day}
                  </button>
                </div>
              {/each}
            </div>
          {/each}
        </div>
      {/key}
    </div>
  </div>

  <div class="card day-card">
    <div class="day-head">
      <h3 class="day-title">{longDate(selected)}</h3>
    </div>

    {#if loadError}
      <p class="status" role="alert">{loadError}</p>
    {/if}

    {#if mode !== "list"}
      {#key formKey}
        <CalendarNoteForm
          seed={formSeed}
          editingId={editingEvent ? editingEvent.id : null}
          repeats={editingEvent !== null && editingEvent.recurrence !== "none"}
          ondone={closeForm}
          oncancel={closeForm}
        />
      {/key}
    {:else}
      {#if listError}
        <p class="status" role="alert">{listError}</p>
      {/if}

      {#if dayNotes.length === 0}
        <p class="empty">No notes</p>
      {:else}
        <ul class="notes">
          {#each dayNotes as note (noteKey(note))}
            <li class="note">
              <span class="text">
                <span class="label">{note.title}</span>
                <span class="meta">
                  {timeLabel(note.start, note.allDay)}{repeatLabel(note) ? ` · ${repeatLabel(note)}` : ""}
                </span>
                {#if note.notes}
                  <span class="body">{note.notes}</span>
                {/if}
              </span>
              {#if confirmingKey === noteKey(note)}
                <span class="confirm">
                  {(seriesOf(note)?.recurrence ?? "none") !== "none" ? "Delete all occurrences?" : "Delete?"}
                </span>
                <button type="button" class="plain" onclick={() => (confirmingKey = null)}>Cancel</button>
                <button type="button" class="destructive" onclick={() => remove(note)}>Delete</button>
              {:else}
                <button type="button" class="plain" onclick={() => startEdit(note)}>Edit</button>
                <button type="button" class="plain" onclick={() => (confirmingKey = noteKey(note))}>
                  Delete
                </button>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}

      <button type="button" class="add" onclick={startAdd}>Add note</button>
    {/if}
  </div>
</div>

<style>
  .card {
    box-sizing: border-box;
    border-radius: 12px;
    background: #111;
  }

  /* The month card. Its side padding is the group padding (16 px), so the grid lines up with the rows. */
  .grid-card {
    padding: 12px 16px 16px;
  }

  .bar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: 32px;
  }

  /* Arrows are 32 px each and the title box is fixed, so the title is centred and nothing moves when the
     month name changes width. */
  .nav {
    display: grid;
    grid-template-columns: 32px 160px 32px;
    align-items: center;
    justify-items: center;
  }

  .arrow {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 16px;
    background: none;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .arrow:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  .arrow svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .title {
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    text-align: center;
    white-space: nowrap;
  }

  .today {
    justify-self: end;
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: 16px;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .today:hover {
    background: rgba(255, 255, 255, 0.16);
  }

  /* Seven equal columns. The labels and the cells use the same 1fr tracks, so they share centres. */
  .weekdays {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    height: 24px;
    margin-top: 8px;
    align-items: center;
    font-size: 11px;
    font-weight: 600;
    text-align: center;
    color: rgba(255, 255, 255, 0.55);
  }

  /* Six rows, always, so the card is the same height in every month. */
  .frame {
    position: relative;
    height: 240px;
    margin-top: 4px;
  }

  .sheet {
    position: absolute;
    inset: 0;
  }

  .week {
    display: grid;
    grid-template-columns: repeat(7, minmax(0, 1fr));
    height: 40px;
  }

  .cell {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 40px;
  }

  .day {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    width: 36px;
    height: 36px;
    padding: 0;
    border: 0;
    border-radius: 18px;
    background: none;
    color: rgba(255, 255, 255, 0.9);
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 1;
    cursor: pointer;
  }

  .day:hover {
    background: rgba(255, 255, 255, 0.08);
  }

  /* A day with notes: the accent at 22% with accent text. On #111 that text is 4.8:1. */
  .day.has {
    background: rgba(255, 106, 61, 0.22);
    color: #ff6a3d;
    font-weight: 600;
  }

  .day.other {
    color: rgba(255, 255, 255, 0.4);
  }

  .day.today {
    box-shadow: inset 0 0 0 1.5px rgba(255, 255, 255, 0.55);
  }

  /* The selected day is solid. Black on #FF6A3D is 7.4:1 (white would be 2.85:1). */
  .day.selected,
  .day.selected:hover {
    background: #ff6a3d;
    color: #000;
    font-weight: 600;
  }

  .day:focus-visible {
    outline: 2px solid #0a84ff;
    outline-offset: 2px;
  }

  .day-card {
    margin-top: 8px;
    overflow: hidden;
  }

  .day-head {
    display: flex;
    align-items: center;
    box-sizing: border-box;
    height: 44px;
    padding: 0 16px;
  }

  .day-title {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    line-height: 17px;
    color: rgba(255, 255, 255, 0.9);
  }

  .empty {
    box-sizing: border-box;
    display: flex;
    align-items: center;
    min-height: 56px;
    margin: 0;
    padding: 0 16px;
    font-size: 15px;
    color: rgba(255, 255, 255, 0.55);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .notes {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .note {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    min-height: 56px;
    padding: 8px 16px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .text {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
  }

  .label,
  .meta,
  .body {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .label {
    font-size: 15px;
    line-height: 20px;
  }

  .meta {
    font-size: 13px;
    line-height: 18px;
    color: rgba(255, 255, 255, 0.55);
    font-variant-numeric: tabular-nums;
  }

  .body {
    font-size: 13px;
    line-height: 18px;
    color: rgba(255, 255, 255, 0.55);
  }

  .confirm {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.55);
  }

  .add {
    display: flex;
    align-items: center;
    box-sizing: border-box;
    width: 100%;
    height: 44px;
    padding: 0 16px;
    border: 0;
    background: none;
    color: #ff6a3d;
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .status {
    margin: 0;
    padding: 8px 16px;
    font-size: 13px;
    line-height: 18px;
    color: #ff453a;
  }

  .plain,
  .destructive {
    height: 32px;
    padding: 0 12px;
    border: 0;
    border-radius: 8px;
    background: none;
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }

  .plain {
    color: rgba(255, 255, 255, 0.85);
  }

  .destructive {
    color: #ff453a;
    font-weight: 600;
  }

  button:focus-visible {
    outline: 2px solid #0a84ff;
    outline-offset: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .arrow,
    .today,
    .day {
      transition: none;
    }
  }
</style>
