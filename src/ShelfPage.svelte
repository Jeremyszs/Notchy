<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { shelf, shelfDrag, shelfConfirmation, refreshShelf } from "./stores/shelf";
  import { config } from "./stores/config";
  import { cubicBezier, relativeAge, wheelRoute, type ShelfItem } from "./lib/shelf";

  export let active = false;
  // Read once when the module loads, as Island.svelte does. Motion is opacity only under reduced motion.
  const reduceMotion = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const ease = cubicBezier(0.2, 0.8, 0.2, 1);
  let now = Date.now();
  let copied: number | null = null;
  let flash: ReturnType<typeof setTimeout>;
  $: if (active) void refreshShelf();
  $: unpinned = $shelf.some((item) => !item.pinned);
  onMount(() => {
    const clock = setInterval(() => { if (active) now = Date.now(); }, 15_000);
    return () => { clearInterval(clock); clearTimeout(flash); };
  });

  async function activate(item: ShelfItem) {
    if (item.missing) return;
    try {
      await invoke(item.kind === "text" ? "shelf_copy" : "shelf_open", { id: item.id });
      if (item.kind === "text") {
        clearTimeout(flash);
        copied = item.id;
        flash = setTimeout(() => copied = null, 900);
      }
    } catch { /* A removed item or a locked clipboard can no longer be activated. */ }
  }
  function action(command: string, args: Record<string, unknown> = {}) {
    void invoke(command, args).catch(() => {});
  }
  function scroll(event: WheelEvent) {
    const route = wheelRoute(true, event.deltaX, event.deltaY);
    event.stopPropagation();
    event.preventDefault();
    const strip = event.currentTarget as HTMLElement;
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? strip.clientWidth : 1;
    strip.scrollLeft += route.delta * unit;
  }

  // A card rises 6 px as it arrives, on the island's curve. Reduced motion keeps a 150 ms fade only.
  function arrive(_node: Element) {
    return reduceMotion
      ? { duration: 150, css: (t: number) => `opacity: ${t}` }
      : { duration: 220, easing: ease, css: (t: number, u: number) => `opacity: ${t}; transform: translateY(${u * 6}px)` };
  }
</script>

<div class="shelf-page">
  <svg class="defs" width="0" height="0" aria-hidden="true">
    <defs>
      <linearGradient id="shelf-aurora-line" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#8CCBFF" />
        <stop offset="0.5" stop-color="#6CB8FF" />
        <stop offset="1" stop-color="#5AAEF7" />
      </linearGradient>
    </defs>
  </svg>
  <header class="head">
    <span class="chip" aria-hidden="true">
      <svg viewBox="0 0 16 16"><path d="M8 2.5v7 M5.2 6.8 8 9.5l2.8-2.7 M3 10.5v2.5h10v-2.5" /></svg>
    </span>
    <span class="title" role="status">{$shelfDrag ? "Drop to shelf" : $shelfConfirmation || "Shelf"}</span>
    <span class="count" aria-label="{$shelf.length} items">{$shelf.length}</span>
    <button class="clear" disabled={!unpinned} onclick={() => action("shelf_clear")}>Clear</button>
    <span class="hairline" aria-hidden="true"><span class="sheen"></span></span>
  </header>

  <div class="strip-wrap">
    <!-- This strip owns wheels and pointer swipes, including at either end. -->
    <div class="shelf-strip" onwheel={scroll} role="group" aria-label="Shelf items">
      {#each $shelf as item (item.id)}
        <div
          class="shelf-card"
          class:text={item.kind === "text"}
          class:file={item.kind === "file"}
          class:missing={item.missing}
          in:arrive
          out:fade={{ duration: 150 }}
        >
          <button class="card-main" aria-label={item.kind === "text" ? `Copy ${item.preview}` : `Open ${item.name}${item.missing ? ", Missing" : ""}`} aria-disabled={item.missing} onclick={() => activate(item)}>
            <span class="kind" aria-hidden="true">
              {#if item.kind === "file"}
                <svg viewBox="0 0 16 16"><path d="M4 2h5l3 3v9H4z M9 2v3h3" /></svg>
              {:else}
                <svg viewBox="0 0 16 16"><path d="M3 4h10 M3 8h10 M3 12h6" /></svg>
              {/if}
            </span>
            {#if item.pinned}
              <span class="pin" aria-label="Pinned">
                <svg viewBox="0 0 16 16"><path d="M5.5 2h5l-.7 3.4 1.9 2.2v1H4.3v-1l1.9-2.2z M8 8.6V14" /></svg>
              </span>
            {/if}
            {#if item.kind === "file"}
              <span class="name">{item.name}</span>
            {:else}
              <span class="preview">{item.preview}</span>
            {/if}
            <span class="foot">
              <span class="age">{item.missing ? "Missing" : relativeAge(item.atMs, now)}</span>
              {#if item.ext}<span class="ext">{item.ext}</span>{/if}
            </span>
            <span class="copied" class:visible={copied === item.id} aria-live="polite">{copied === item.id ? "Copied" : ""}</span>
          </button>
          <div class="actions">
            <button aria-label={item.pinned ? "Unpin item" : "Pin item"} title={item.pinned ? "Unpin" : "Pin"} onclick={() => action("shelf_pin", { id: item.id, pinned: !item.pinned })}>
              <svg viewBox="0 0 16 16"><path d="M5.5 2h5l-.7 3.4 1.9 2.2v1H4.3v-1l1.9-2.2z M8 8.6V14" /></svg>
            </button>
            {#if item.kind === "file"}
              <button aria-label="Show in Explorer" title="Show in Explorer" onclick={() => action("shelf_reveal", { id: item.id })}>
                <svg viewBox="0 0 16 16"><path d="M2 4h4.5l1.5 1.5H14v7H2z" /></svg>
              </button>
            {/if}
            <button aria-label="Remove item" title="Remove" onclick={() => action("shelf_remove", { id: item.id })}>
              <svg viewBox="0 0 16 16"><path d="m4.5 4.5 7 7 M11.5 4.5l-7 7" /></svg>
            </button>
          </div>
        </div>
      {/each}
    </div>

    {#if $shelf.length === 0}
      <div class="empty">
        <span class="sheen" aria-hidden="true"></span>
        <svg class="tray" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v11 M7.5 9.5 12 14l4.5-4.5 M4 14v5.5h16V14" /></svg>
        <p class="empty-title">Drop files or copy text</p>
        <p class="empty-sub">{!$config.shelf.clipboard && $config.shelf.files ? "Clipboard history is off, turn it on in Settings" : "Copied text stays in memory"}</p>
      </div>
    {/if}

    {#if $shelfDrag}
      <div class="drop-fade" aria-hidden="true" in:fade={{ duration: 150 }} out:fade={{ duration: 150 }}>
        <div class="drop">
          <span class="sheen"></span>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .defs { position: absolute; }
  .shelf-page {
    /* One accent: a soft sky blue, in three tints for the gradient motion. */
    --accent-a: #8ccbff;
    --accent-b: #6cb8ff;
    --accent-c: #5aaef7;
    width: 100%;
    height: 146px;
    display: flex;
    flex-direction: column;
    justify-content: center;
    font-size: 11px;
    line-height: 14px;
    color: rgb(255 255 255 / 90%);
  }
  button { color: inherit; font: inherit; border: 0; cursor: pointer; }
  button:focus-visible { outline: 2px solid rgb(255 255 255 / 70%); outline-offset: 2px; }
  .sheen {
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 300%;
    background: linear-gradient(90deg, var(--accent-b), var(--accent-a), var(--accent-c), var(--accent-b)) 0 0 / 33.333% 100% repeat-x;
    animation: sheen 12s linear infinite;
    pointer-events: none;
  }
  @keyframes sheen { from { transform: translateX(0); } to { transform: translateX(-33.333%); } }

  /* Header: 20 px, then an 8 px gap to the strip. The page centres 132 px in 146 px, so 7 px each side. */
  .head { position: relative; height: 20px; display: flex; align-items: center; gap: 6px; }
  .chip {
    width: 20px;
    height: 20px;
    flex: 0 0 20px;
    display: grid;
    place-items: center;
    border-radius: 6px;
    background: linear-gradient(135deg, var(--accent-a), var(--accent-b));
  }
  .chip svg { width: 12px; height: 12px; fill: none; stroke: rgb(255 255 255 / 92%); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .title { font-size: 12px; line-height: 16px; font-weight: 600; color: rgb(255 255 255 / 90%); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .count {
    margin-left: auto;
    min-width: 18px;
    height: 16px;
    padding: 0 5px;
    border-radius: 8px;
    display: grid;
    place-items: center;
    background: rgb(108 184 255 / 16%);
    font-size: 11px;
    font-weight: 600;
    color: rgb(255 255 255 / 60%);
    font-variant-numeric: tabular-nums;
  }
  .clear { background: none; padding: 2px 4px; font-size: 11px; font-weight: 600; color: rgb(255 255 255 / 60%); border-radius: 6px; }
  .clear:hover:not(:disabled) { color: rgb(255 255 255 / 90%); }
  .clear:disabled { opacity: 0.35; cursor: default; }
  .hairline { position: absolute; left: 0; right: 0; bottom: -4px; height: 1px; overflow: hidden; opacity: 0.45; }
  .hairline .sheen { height: 100%; }

  /* The strip is 104 px, with the same card height, so the content block is exactly 132 px. */
  .strip-wrap { position: relative; height: 104px; margin-top: 8px; }
  .shelf-strip { position: absolute; inset: 0; display: flex; gap: 8px; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; scroll-behavior: auto; }
  .shelf-strip::-webkit-scrollbar { display: none; }

  .shelf-card {
    position: relative;
    width: 96px;
    height: 104px;
    flex: 0 0 96px;
    border-radius: 12px;
    background: linear-gradient(rgb(108 184 255 / 10%), rgb(108 184 255 / 10%)), rgb(255 255 255 / 8%);
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 6%);
  }
  .shelf-card:hover, .shelf-card:focus-within { transform: translateY(-1px); }
  .shelf-card.missing .card-main { opacity: 0.45; }
  .card-main {
    position: absolute;
    inset: 0;
    padding: 0 10px;
    background: none;
    border-radius: 12px;
    text-align: left;
    overflow: hidden;
  }
  .kind, .pin { position: absolute; top: 10px; width: 12px; height: 12px; }
  .kind { left: 10px; color: rgb(255 255 255 / 85%); }
  .kind svg, .pin svg { display: block; width: 12px; height: 12px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }
  .pin { right: 10px; }
  .pin svg { stroke: url(#shelf-aurora-line); }
  .preview, .name { position: absolute; top: 26px; left: 10px; right: 10px; display: -webkit-box; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .preview { -webkit-line-clamp: 3; line-clamp: 3; white-space: pre-wrap; }
  .name { -webkit-line-clamp: 2; line-clamp: 2; font-size: 11px; line-height: 14px; font-weight: 600; }
  .foot { position: absolute; left: 10px; right: 10px; bottom: 10px; height: 14px; display: flex; align-items: center; gap: 6px; transition: opacity 150ms ease; }
  .age { color: rgb(255 255 255 / 60%); font-variant-numeric: tabular-nums; }
  .ext { margin-left: auto; max-width: 40px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: rgb(255 255 255 / 60%); font-weight: 600; }

  .copied {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(12 12 12 / 72%);
    border-radius: 12px;
    font-weight: 600;
    color: rgb(255 255 255 / 90%);
    opacity: 0;
    pointer-events: none;
    transition: opacity 150ms ease;
  }
  .copied.visible { opacity: 1; }

  /* Hover and focus swap the footer for a dark glass pill of actions. Each action is a real button. */
  .actions {
    position: absolute;
    left: 50%;
    bottom: 6px;
    transform: translateX(-50%);
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: rgb(12 12 12 / 88%);
    border: 1px solid rgb(255 255 255 / 10%);
    opacity: 0;
    pointer-events: none;
    transition: opacity 150ms ease;
  }
  .shelf-card:hover .actions, .shelf-card:focus-within .actions { opacity: 1; pointer-events: auto; }
  .shelf-card:hover .foot, .shelf-card:focus-within .foot { opacity: 0; }
  .actions button { width: 22px; height: 20px; display: grid; place-items: center; border-radius: 999px; background: none; color: rgb(255 255 255 / 90%); }
  .actions button:hover { background: rgb(255 255 255 / 14%); }
  .actions svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }

  /* The empty state and the drop highlight share the sheen, which moves by transform only. */
  .empty {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 14px;
    border: 1px dashed rgb(255 255 255 / 22%);
    overflow: hidden;
    text-align: center;
  }
  .empty .sheen { opacity: 0.12; animation-duration: 14s; }
  .tray { width: 22px; height: 22px; fill: none; stroke: rgb(255 255 255 / 85%); stroke-width: 1.5; stroke-linecap: round; stroke-linejoin: round; margin-bottom: 4px; position: relative; }
  .empty-title { position: relative; margin: 0; font-size: 12px; line-height: 16px; font-weight: 600; color: rgb(255 255 255 / 90%); }
  .empty-sub { position: relative; margin: 0; font-size: 11px; line-height: 14px; color: rgb(255 255 255 / 60%); }

  .drop-fade { position: absolute; inset: -3px; pointer-events: none; }
  .drop {
    position: absolute;
    inset: 0;
    border-radius: 15px;
    border: 1.5px dashed rgb(140 203 255 / 70%);
    overflow: hidden;
    pointer-events: none;
    animation: pulse 3.5s ease-in-out infinite;
  }
  .drop .sheen { opacity: 0.3; animation-duration: 3.5s; }
  @keyframes pulse { 0%, 100% { opacity: 0.75; } 50% { opacity: 1; } }

  @media (prefers-reduced-motion: reduce) {
    .sheen, .drop { animation: none; }
    .shelf-card, .shelf-card:hover, .shelf-card:focus-within { transform: none; transition: none; }
    .copied, .actions, .foot { transition: none; }
  }
</style>
