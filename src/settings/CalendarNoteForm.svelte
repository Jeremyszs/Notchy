<script lang="ts">
  // The note form under the selected day: add a note on that day, or edit a note. The date is the
  // selected day, shown as a label, when adding. Editing shows a date input, so the day can change.
  // Each field is checked here as well as by the backend. Saving reaches both windows through
  // calendar://update, so the grid refreshes itself.
  import { invoke } from "@tauri-apps/api/core";
  import {
    durationFrom,
    eventInput,
    hasErrors,
    leadLabel,
    LENGTH_OPTIONS,
    lengthChoiceOf,
    MAX_DURATION_MINUTES,
    REMIND_MAX,
    REMIND_OPTIONS,
    REPEAT_OPTIONS,
    reminderChipLabel,
    toggleReminder,
    validateForm,
    type EventForm,
    type FormErrors,
  } from "../lib/calendar.ts";
  import { longDate } from "../lib/month.ts";
  import { config } from "../stores/config";
  import Switch from "./Switch.svelte";

  /** The form to start from. It is copied, so the parent keeps its own value. */
  export let seed: EventForm;
  /** The event being edited, or null when adding. */
  export let editingId: string | null;
  /** Whether the event repeats, so the edit form can say that every occurrence changes. */
  export let repeats = false;
  export let ondone: () => void;
  export let oncancel: () => void;

  let form: EventForm = { ...seed };
  let errors: FormErrors = {};
  let formError = "";
  let busy = false;
  // The length field's choice ("none", the minutes, or "custom") and the typed minutes for "custom".
  let lengthChoice = lengthChoiceOf(form.durationMinutes);
  let customLength = form.durationMinutes === null ? "" : String(form.durationMinutes);
  $: form.durationMinutes = durationFrom(lengthChoice, customLength);
  // "Default" is the Settings lead time, which is what a null list means.
  $: defaultOn = form.remindBeforeMinutes === null;

  // Turns the Settings default on, or off to an empty list (the start only) when it is on.
  function toggleDefault(): void {
    form.remindBeforeMinutes = defaultOn ? [] : null;
  }

  function pickRemind(minutes: number): void {
    form.remindBeforeMinutes = toggleReminder(form.remindBeforeMinutes, minutes);
  }

  async function save(): Promise<void> {
    if (busy) return;
    errors = validateForm(form);
    if (hasErrors(errors)) return;
    busy = true;
    formError = "";
    try {
      await invoke("calendar_upsert", { input: eventInput(form, editingId) });
      ondone();
    } catch (error) {
      formError = String(error);
    } finally {
      busy = false;
    }
  }
</script>

<form
  class="form"
  novalidate
  onsubmit={(event) => {
    event.preventDefault();
    void save();
  }}
>
  <div class="field">
    <label for="note-title">Title</label>
    <input
      id="note-title"
      type="text"
      maxlength="200"
      placeholder="Note title"
      autocomplete="off"
      aria-invalid={errors.title ? "true" : undefined}
      aria-describedby={errors.title ? "note-title-error" : undefined}
      bind:value={form.title}
    />
    {#if errors.title}
      <p id="note-title-error" class="field-error" role="alert">{errors.title}</p>
    {/if}
  </div>

  <div class="field">
    <label for="note-notes">Notes</label>
    <textarea id="note-notes" rows="2" maxlength="2000" bind:value={form.notes}></textarea>
  </div>

  {#if editingId === null}
    <div class="inline">
      <span class="inline-label">Date</span>
      <span class="fixed-date">{longDate(form.date)}</span>
    </div>
  {:else}
    <div class="field">
      <label for="note-date">Date</label>
      <input
        id="note-date"
        type="date"
        aria-invalid={errors.date ? "true" : undefined}
        bind:value={form.date}
      />
      {#if errors.date}
        <p class="field-error" role="alert">{errors.date}</p>
      {/if}
      {#if repeats}
        <p class="hint">A repeating note changes every occurrence.</p>
      {/if}
    </div>
  {/if}

  <div class="field-row">
    <div class="field">
      <label for="note-time">Time</label>
      <input
        id="note-time"
        type="time"
        disabled={form.allDay}
        aria-invalid={errors.time ? "true" : undefined}
        bind:value={form.time}
      />
      {#if errors.time}
        <p class="field-error" role="alert">{errors.time}</p>
      {/if}
    </div>
    <div class="field">
      <label for="note-repeat">Repeat</label>
      <select id="note-repeat" bind:value={form.recurrence}>
        {#each REPEAT_OPTIONS as option (option.value)}
          <option value={option.value}>{option.label}</option>
        {/each}
      </select>
    </div>
  </div>

  {#if !form.allDay}
    <div class="field-row">
      <div class="field">
        <label for="note-length">Length</label>
        <select
          id="note-length"
          aria-invalid={errors.duration ? "true" : undefined}
          bind:value={lengthChoice}
        >
          <option value="none">No length</option>
          {#each LENGTH_OPTIONS as minutes (minutes)}
            <option value={String(minutes)}>{leadLabel(minutes)}</option>
          {/each}
          <option value="custom">Custom</option>
        </select>
      </div>
      {#if lengthChoice === "custom"}
        <div class="field">
          <label for="note-length-custom">Minutes</label>
          <input
            id="note-length-custom"
            type="number"
            inputmode="numeric"
            min="1"
            max={MAX_DURATION_MINUTES}
            step="1"
            aria-invalid={errors.duration ? "true" : undefined}
            value={customLength}
            oninput={(event) => (customLength = event.currentTarget.value)}
          />
        </div>
      {/if}
    </div>
    {#if errors.duration}
      <p class="field-error" role="alert">{errors.duration}</p>
    {/if}

    <div class="field">
      <span class="field-label" id="note-remind-label">Remind me</span>
      <div class="chips" role="group" aria-labelledby="note-remind-label">
        <button
          type="button"
          class="chip"
          aria-pressed={defaultOn}
          onclick={toggleDefault}
        >
          Default
        </button>
        {#each REMIND_OPTIONS as minutes (minutes)}
          {@const picked = form.remindBeforeMinutes?.includes(minutes) ?? false}
          <button
            type="button"
            class="chip"
            aria-pressed={picked}
            disabled={!picked && (form.remindBeforeMinutes?.length ?? 0) >= REMIND_MAX}
            onclick={() => pickRemind(minutes)}
          >
            {reminderChipLabel(minutes)}
          </button>
        {/each}
      </div>
      {#if defaultOn}
        <p class="hint">{leadLabel($config.calendar.leadMinutes)} before, as set in Settings.</p>
      {:else if form.remindBeforeMinutes?.length === 0}
        <p class="hint">Only when it starts.</p>
      {:else}
        <p class="hint">Up to {REMIND_MAX} reminders.</p>
      {/if}
    </div>
  {/if}

  <div class="inline">
    <span class="inline-label">All day</span>
    <Switch
      checked={form.allDay}
      label="All day"
      onchange={(on) => {
        form.allDay = on;
      }}
    />
  </div>

  {#if formError}
    <p class="field-error" role="alert">{formError}</p>
  {/if}

  <div class="actions">
    <button type="button" class="plain" onclick={oncancel}>Cancel</button>
    <button type="submit" class="strong" disabled={busy}>
      {editingId === null ? "Add note" : "Save"}
    </button>
  </div>
</form>

<style>
  .form {
    display: flex;
    flex-direction: column;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 16px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .field label,
  .field-label {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.55);
  }

  /* The reminder choices are toggles: off is 10% white, on is white with a black label. */
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    height: 28px;
    padding: 0 10px;
    border: 0;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    font: inherit;
    font-size: 13px;
    line-height: 16px;
    cursor: pointer;
  }

  .chip[aria-pressed="true"] {
    background: #fff;
    color: #000;
    font-weight: 600;
  }

  .chip:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* Two columns that can shrink below their inputs' native width, so the pair stays in the group. */
  .field-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .field-row .field + .field {
    box-shadow: inset 1px 0 0 rgba(255, 255, 255, 0.08);
  }

  input,
  textarea,
  select {
    box-sizing: border-box;
    min-width: 0;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
    font: inherit;
    font-size: 15px;
    color-scheme: dark;
  }

  input[type="date"],
  input[type="time"] {
    font-variant-numeric: tabular-nums;
  }

  input:disabled {
    opacity: 0.4;
  }

  textarea {
    resize: vertical;
    min-height: 44px;
    line-height: 20px;
  }

  input[aria-invalid="true"] {
    box-shadow: inset 0 0 0 1px #ff453a;
  }

  .field-error {
    margin: 0;
    font-size: 13px;
    line-height: 18px;
    color: #ff453a;
  }

  .hint {
    margin: 0;
    font-size: 13px;
    line-height: 18px;
    color: rgba(255, 255, 255, 0.55);
  }

  .inline {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    box-sizing: border-box;
    min-height: 56px;
    padding: 0 16px;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .inline-label {
    font-size: 15px;
  }

  .fixed-date {
    font-size: 15px;
    font-variant-numeric: tabular-nums;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    box-sizing: border-box;
    height: 56px;
    padding: 0 16px;
    align-items: center;
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .plain,
  .strong {
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

  /* The calendar accent. White on #FF6A3D is 2.85:1, so the label is weight 600 to read at 15 px. */
  .strong {
    background: #ff6a3d;
    color: #fff;
    font-weight: 600;
  }

  .strong:disabled {
    opacity: 0.4;
    cursor: default;
  }

  button:focus-visible,
  input:focus-visible,
  textarea:focus-visible,
  select:focus-visible {
    outline: 2px solid #0a84ff;
    outline-offset: 2px;
  }
</style>
