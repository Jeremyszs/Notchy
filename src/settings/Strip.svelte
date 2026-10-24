<script lang="ts">
  import { tick, type Snippet } from "svelte";
  import { flip } from "svelte/animate";
  import { SMOOTH_MS, smoothEase } from "../lib/motion";
  import { moveId, nearestSlot } from "./rows";

  // A row of items in the order the notch shows them. Drag one to a new place (the others make room
  // as it passes), or focus it and use the arrow keys. Each item has a remove badge. The item's
  // look comes from the `chip` snippet, so pages and controls share the behaviour.
  let {
    ids,
    label,
    labelOf,
    chip,
    canRemove = () => true,
    onreorder,
    onremove,
    announce,
  }: {
    ids: string[];
    label: string;
    labelOf: (id: string) => string;
    chip: Snippet<[string, number]>;
    canRemove?: (id: string) => boolean;
    onreorder: (ids: string[]) => void;
    onremove: (id: string) => void;
    announce: (text: string) => void;
  } = $props();

  // A press becomes a drag once the pointer has moved this far, so a click on the badge never drags.
  const DRAG_START_PX = 4;
  const reduceMotion =
    typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  let list: HTMLElement;
  let press: { id: string; x: number; y: number; dx: number; dy: number; w: number; h: number } | null = null;
  let dragId: string | null = $state(null);
  let order: string[] | null = $state(null);
  let ghost = $state({ x: 0, y: 0, w: 0, h: 0 });
  let slots: { x: number; y: number }[] = [];

  let shown = $derived(order ?? ids);

  function down(event: PointerEvent, id: string) {
    if (event.button !== 0) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    press = { id, x: event.clientX, y: event.clientY, dx: event.clientX - box.left, dy: event.clientY - box.top, w: box.width, h: box.height };
  }

  function move(event: PointerEvent) {
    if (!press) return;
    if (!dragId) {
      if (Math.hypot(event.clientX - press.x, event.clientY - press.y) < DRAG_START_PX) return;
      // Slot centres are read once: items are one size, so the slots stay put while items move.
      slots = [...list.querySelectorAll<HTMLElement>(":scope > .slot")].map((node) => {
        const box = node.getBoundingClientRect();
        return { x: box.left + box.width / 2, y: box.top + box.height / 2 };
      });
      dragId = press.id;
      order = ids;
    }
    ghost = { x: event.clientX - press.dx, y: event.clientY - press.dy, w: press.w, h: press.h };
    const to = nearestSlot(slots, event.clientX, event.clientY);
    if (to >= 0 && order) order = moveId(order, press.id, to);
  }

  function up() {
    const finished = order;
    const id = dragId;
    press = null;
    dragId = null;
    order = null;
    if (id && finished && finished.join() !== ids.join()) {
      onreorder(finished);
      announce(`${labelOf(id)} moved to position ${finished.indexOf(id) + 1} of ${finished.length}`);
    }
  }

  async function key(event: KeyboardEvent, id: string) {
    const index = ids.indexOf(id);
    if (event.key === "Delete" || event.key === "Backspace") {
      if (!canRemove(id)) return;
      event.preventDefault();
      onremove(id);
      return;
    }
    const step = { ArrowLeft: -1, ArrowUp: -1, ArrowRight: 1, ArrowDown: 1 }[event.key];
    if (!step) return;
    event.preventDefault();
    const next = moveId(ids, id, index + step);
    if (next === ids) return;
    onreorder(next);
    announce(`${labelOf(id)} moved to position ${index + step + 1} of ${ids.length}`);
    await tick();
    list.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"] .handle`)?.focus();
  }
</script>

<!-- The drag is followed on the window: a keyed item that moves in the DOM can lose pointer capture. -->
<svelte:window onpointermove={move} onpointerup={up} onpointercancel={up} />

<div class="strip" role="list" aria-label={label} bind:this={list}>
  {#each shown as id, index (id)}
    <div
      class="slot"
      class:placeholder={dragId === id}
      role="listitem"
      data-id={id}
      animate:flip={{ duration: reduceMotion ? 0 : SMOOTH_MS, easing: smoothEase }}
    >
      <button
        type="button"
        class="handle"
        aria-label="{labelOf(id)}, position {index + 1} of {shown.length}"
        aria-describedby="strip-help"
        onpointerdown={(event) => down(event, id)}
        onkeydown={(event) => key(event, id)}
      >
        {@render chip(id, index)}
      </button>
      {#if canRemove(id)}
        <button type="button" class="remove" aria-label="Remove {labelOf(id)}" title="Remove" onclick={() => onremove(id)}>
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 6h6" /></svg>
        </button>
      {/if}
    </div>
  {/each}
</div>

{#if dragId}
  <div class="ghost" style="left:{ghost.x}px;top:{ghost.y}px;width:{ghost.w}px;height:{ghost.h}px" aria-hidden="true">
    {@render chip(dragId, shown.indexOf(dragId))}
  </div>
{/if}

<style>
  .strip {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px;
    min-height: 40px;
  }

  .slot {
    position: relative;
  }

  .handle {
    display: block;
    padding: 0;
    border: 0;
    border-radius: 12px;
    background: none;
    color: inherit;
    font: inherit;
    cursor: grab;
    touch-action: none;
    transition:
      transform var(--spring-snappy-ms) var(--spring-snappy),
      opacity 150ms ease;
  }

  .handle:hover {
    transform: translateY(-2px);
  }

  .handle:active {
    cursor: grabbing;
  }

  /* The dragged item's place stays open as a dashed outline while the copy follows the pointer. */
  .placeholder .handle {
    opacity: 0;
  }

  .placeholder::before {
    content: "";
    position: absolute;
    inset: 0;
    border: 1.5px dashed rgb(255 255 255 / 25%);
    border-radius: 12px;
  }

  .placeholder .remove {
    display: none;
  }

  .ghost {
    position: fixed;
    z-index: 10;
    pointer-events: none;
    transform: scale(1.06) rotate(-1.5deg);
    filter: drop-shadow(0 10px 18px rgb(0 0 0 / 55%));
  }

  .remove {
    position: absolute;
    top: -6px;
    left: -6px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: #3a3a3c;
    box-shadow: 0 0 0 2px #000;
    color: #fff;
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease;
  }

  .slot:hover .remove,
  .slot:focus-within .remove {
    opacity: 1;
  }

  .remove:hover {
    background: #ff453a;
  }

  .remove svg {
    width: 12px;
    height: 12px;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
  }

  @media (prefers-reduced-motion: reduce) {
    .handle,
    .handle:hover {
      transition: none;
      transform: none;
    }

    .ghost {
      transform: none;
    }
  }
</style>
