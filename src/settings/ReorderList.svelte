<script lang="ts">
  import { tick } from "svelte";
  import Switch from "./Switch.svelte";
  import {
    dropIndex,
    rowOffset,
    settleOffset,
    type Drag,
    type Row,
    type Settle,
  } from "./rows";

  export let label: string;
  export let rows: Row[];
  export let ontoggle: (id: string, on: boolean) => void;
  export let onmove: (id: string, to: number) => void;

  let list: HTMLUListElement;

  // The row under the pointer. `startY` is only needed while the pointer is down.
  let drag: (Drag & { startY: number }) | null = null;
  // After a drop the moved row holds its visual place until the next render, then glides to rest.
  let settle: Settle | null = null;

  // Each row's offset, read in this reactive statement so every change to drag or settle
  // reaches the style. A call made only inside the markup would not re-run on those changes.
  $: offsets = rows.map((row, index) => rowOffset(row.id, index, drag, settle));

  function grab(event: PointerEvent, index: number): void {
    if (event.button !== 0) return;
    (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
    drag = { id: rows[index].id, from: index, to: index, startY: event.clientY, dy: 0 };
  }

  function carry(event: PointerEvent): void {
    if (!drag) return;
    const dy = event.clientY - drag.startY;
    drag = { ...drag, dy, to: dropIndex(drag.from, dy, rows.length) };
  }

  function drop(): void {
    if (!drag) return;
    const { id, from, to, dy } = drag;
    drag = null;
    if (to === from) return;
    // The row keeps the offset it had under the pointer, measured from its new slot.
    const next: Settle = { id, y: settleOffset(dy, from, to) };
    settle = next;
    onmove(id, to);
    // Once the offset is in the DOM, a forced style pass records it as the start of the glide.
    // Clearing it then lets the row transition from there into its slot. A newer drop keeps its own.
    void tick().then(() => {
      void list.getBoundingClientRect();
      if (settle === next) settle = null;
    });
  }

  function cancel(): void {
    drag = null;
  }

  // Alt with Up or Down moves the row whose handle has focus, and focus follows it.
  function nudge(event: KeyboardEvent, index: number): void {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    const to = index + (event.key === "ArrowUp" ? -1 : 1);
    if (to < 0 || to >= rows.length) return;
    const id = rows[index].id;
    onmove(id, to);
    void tick().then(() =>
      list
        .querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"] .grip`)
        ?.focus(),
    );
  }
</script>

<ul class="group" aria-label={label} bind:this={list}>
  {#each rows as row, index (row.id)}
    <li
      class="row"
      class:dim={row.dim}
      class:still={settle !== null || drag?.id === row.id}
      class:lifted={drag?.id === row.id}
      data-id={row.id}
      style="transform: translateY({offsets[index]}px)"
    >
      <button
        type="button"
        class="grip"
        aria-label="Move {row.label}"
        title="Drag to reorder. Alt+Up or Alt+Down moves it."
        onkeydown={(event) => nudge(event, index)}
        onpointerdown={(event) => grab(event, index)}
        onpointermove={carry}
        onpointerup={drop}
        onpointercancel={cancel}
        onlostpointercapture={cancel}
      >
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" />
        </svg>
      </button>
      <span class="text">
        <span class="label">{row.label}</span>
        {#if row.note}
          <span class="note">{row.note}</span>
        {/if}
      </span>
      <Switch
        checked={row.on}
        disabled={!row.enabled}
        label={row.label}
        onchange={(on) => ontoggle(row.id, on)}
      />
    </li>
  {/each}
</ul>

<style>
  .group {
    margin: 0;
    padding: 0;
    overflow: hidden;
    list-style: none;
    border-radius: 12px;
    background: #111;
  }

  /* Rows are a fixed height, so a drag can work in whole row steps. Keep it in step with ROW_HEIGHT. */
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    box-sizing: border-box;
    height: 56px;
    padding: 0 16px 0 4px;
    background: #111;
    transition: transform 180ms cubic-bezier(0.2, 0.7, 0.3, 1);
  }

  .row + .row::before {
    content: "";
    position: absolute;
    top: 0;
    left: 48px;
    right: 0;
    height: 1px;
    background: rgba(255, 255, 255, 0.08);
  }

  .row.still {
    transition: none;
  }

  .row.lifted {
    z-index: 1;
    background: #1c1c1e;
  }

  .row.dim {
    opacity: 0.45;
  }

  .grip {
    flex: none;
    display: grid;
    place-items: center;
    width: 40px;
    height: 44px;
    margin: 0;
    padding: 0;
    border: 0;
    border-radius: 8px;
    background: none;
    color: rgba(255, 255, 255, 0.4);
    cursor: grab;
    touch-action: none;
  }

  .lifted .grip {
    cursor: grabbing;
  }

  .grip svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 3;
    stroke-linecap: round;
  }

  .grip:focus-visible {
    outline: 2px solid #0a84ff;
    outline-offset: -2px;
  }

  .text {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .label {
    overflow: hidden;
    font-size: 15px;
    line-height: 20px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .note {
    overflow: hidden;
    font-size: 12px;
    line-height: 16px;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.55);
  }

  @media (prefers-reduced-motion: reduce) {
    .row {
      transition: none;
    }
  }
</style>
