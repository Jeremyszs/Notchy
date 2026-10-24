<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { flip } from "svelte/animate";
  import { rise, SMOOTH_MS, smoothEase } from "./lib/motion";
  import { invoke } from "@tauri-apps/api/core";
  import { shelf, shelfDrag, shelfConfirmation, shelfMode, refreshShelf } from "./stores/shelf";
  import { config } from "./stores/config";
  import {
    cubicBezier,
    defaultMode,
    DRAG_START_PX,
    FAMILY_COLORS,
    fileFamily,
    movedBeyond,
    relativeAge,
    wheelRoute,
    type ShelfItem,
    type ShelfMode,
  } from "./lib/shelf";

  export let active = false;
  // Read once when the module loads, as Island.svelte does. Motion is opacity only under reduced motion.
  const reduceMotion = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const ease = cubicBezier(0.2, 0.8, 0.2, 1);
  // A removed card leaves in 180 ms. A clear staggers its cards 35 ms apart, and the last one starts by 240 ms.
  const LEAVE_MS = 180;
  const STAGGER_MS = 35;
  const STAGGER_CAP_MS = 240;
  const LEAVE_FRAMES: Keyframe[] = [
    { opacity: 1, transform: "translateY(0) scale(1)" },
    { opacity: 0, transform: "translateY(-6px) scale(0.92)" },
  ];

  let now = Date.now();
  let copied: number | null = null;
  let flash: ReturnType<typeof setTimeout>;
  let stage: HTMLElement;
  let tabEls: Record<ShelfMode, HTMLButtonElement | undefined> = { file: undefined, text: undefined };
  let busy = false;
  const removing = new Set<number>();
  let press: { item: ShelfItem; x: number; y: number } | null = null;
  let dragging: number | null = null;
  // A drag leaves no click behind when the button is released outside the page. A press resets this.
  let dragged = false;

  $: if (active) void refreshShelf();
  $: files = $config.shelf.files;
  $: texts = $config.shelf.clipboard;
  $: mode = $shelfMode ?? defaultMode(texts, files);
  // A mode whose switch is off shows nothing, so its own empty state explains why.
  $: fileItems = files ? $shelf.filter((item) => item.kind === "file") : [];
  $: textItems = texts ? $shelf.filter((item) => item.kind === "text") : [];
  $: shown = mode === "file" ? fileItems : textItems;
  $: canClear = shown.some((item) => !item.pinned);
  $: status = $shelfDrag ? "Drop to shelf" : $shelfConfirmation;

  onMount(() => {
    const clock = setInterval(() => { if (active) now = Date.now(); }, 15_000);
    return () => { clearInterval(clock); clearTimeout(flash); };
  });

  function choose(next: ShelfMode) {
    shelfMode.set(next);
  }
  // Left and Right move between the two modes, as in a native tab list.
  function tabKey(event: KeyboardEvent) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next: ShelfMode = event.key === "ArrowLeft" ? "file" : "text";
    choose(next);
    tabEls[next]?.focus();
  }

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

  // Shared by the removed card and the cleared cards. Only opacity, scale and lift animate.
  function leave(node: Element, delay: number): Animation {
    return node.animate(LEAVE_FRAMES, { duration: LEAVE_MS, delay, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)", fill: "forwards" });
  }

  // The card animates first and the shelf is changed after it, so the cards after it slide in only once it is gone.
  async function remove(item: ShelfItem, node: Element | null) {
    if (removing.has(item.id)) return;
    removing.add(item.id);
    try {
      if (node && !reduceMotion) await leave(node, 0).finished;
      await invoke("shelf_remove", { id: item.id });
    } catch {
      node?.getAnimations().forEach((animation) => animation.cancel());
    } finally {
      removing.delete(item.id);
    }
  }

  async function clearMode() {
    if (!canClear || busy) return;
    busy = true;
    const kind = mode;
    const cards = reduceMotion ? [] : [...stage.querySelectorAll<HTMLElement>('.shelf-card[data-unpinned="true"]')];
    const leaving = cards.map((node, index) => leave(node, Math.min(index * STAGGER_MS, STAGGER_CAP_MS)));
    try {
      await Promise.all(leaving.map((animation) => animation.finished));
      await invoke("shelf_clear", { kind });
    } catch {
      leaving.forEach((animation) => animation.cancel());
    } finally {
      busy = false;
    }
  }

  // A press on a card activates it on release. Moving past the threshold starts a drag out instead.
  function pressCard(event: PointerEvent, item: ShelfItem) {
    if (item.missing || event.button !== 0) return;
    press = { item, x: event.clientX, y: event.clientY };
    dragged = false;
  }
  function moveCard(event: PointerEvent) {
    if (!press || dragging !== null) return;
    if (!movedBeyond(press.x, press.y, event.clientX, event.clientY, DRAG_START_PX)) return;
    const { item } = press;
    press = null;
    if (!$shelf.some((existing) => existing.id === item.id)) return;
    dragged = true;
    dragging = item.id;
    // Copies the item into the app under the cursor. Resolves when the drag ends, however it ends.
    invoke("shelf_drag_out", { id: item.id })
      .catch(() => {})
      .finally(() => { dragging = null; });
  }
  function activateCard(item: ShelfItem) {
    if (dragged) return;
    void activate(item);
  }

  function scroll(event: WheelEvent) {
    const route = wheelRoute(true, event.deltaX, event.deltaY);
    event.stopPropagation();
    event.preventDefault();
    const strip = event.currentTarget as HTMLElement;
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? strip.clientWidth : 1;
    strip.scrollLeft += route.delta * unit;
  }

  // A card rises 6 px out of a light blur as it arrives, on the smooth spring. Reduced motion keeps a
  // 150 ms fade only.
  function arrive(node: Element) {
    return rise(node, { y: 6, blur: 2, reduced: reduceMotion });
  }

  // A new mode's row slides in from the side its tab is on: Text from the right, Files from the left.
  function modeIn(_node: Element) {
    if (reduceMotion) return { duration: 160, css: (t: number) => `opacity: ${t}` };
    const side = mode === "text" ? 1 : -1;
    return {
      duration: SMOOTH_MS,
      easing: smoothEase,
      css: (t: number, u: number) => `opacity: ${Math.min(1, t * 1.6)}; transform: translateX(${u * side * 14}px)`,
    };
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

  <div class="switch">
    <div class="tabs" role="tablist" aria-label="Shelf mode" data-mode={mode} class:away={status !== ""}>
      <span class="pill" aria-hidden="true"></span>
      <button
        bind:this={tabEls.file}
        role="tab"
        id="shelf-tab-file"
        aria-selected={mode === "file"}
        aria-controls="shelf-panel"
        tabindex={mode === "file" ? 0 : -1}
        class:selected={mode === "file"}
        onclick={() => choose("file")}
        onkeydown={tabKey}
      >Files <span class="count">{fileItems.length}</span></button>
      <button
        bind:this={tabEls.text}
        role="tab"
        id="shelf-tab-text"
        aria-selected={mode === "text"}
        aria-controls="shelf-panel"
        tabindex={mode === "text" ? 0 : -1}
        class:selected={mode === "text"}
        onclick={() => choose("text")}
        onkeydown={tabKey}
      >Text <span class="count">{textItems.length}</span></button>
    </div>
    {#if status}
      <p class="status" role="status" transition:fade={{ duration: 150 }}>{status}</p>
    {/if}
  </div>

  <div class="stage" bind:this={stage}>
    {#key mode}
      <div
        id="shelf-panel"
        class="layer"
        role="tabpanel"
        aria-labelledby="shelf-tab-{mode}"
        in:modeIn
        out:fade={{ duration: 120 }}
      >
        <!-- This strip owns wheels and pointer swipes, including at either end. -->
        <div class="shelf-strip" onwheel={scroll} role="group" aria-label="{mode === 'file' ? 'Files' : 'Text'} on the shelf">
          {#each shown as item (item.id)}
            <div
              class="shelf-card"
              class:text={item.kind === "text"}
              class:file={item.kind === "file"}
              class:missing={item.missing}
              class:dragging={dragging === item.id}
              data-id={item.id}
              data-unpinned={!item.pinned}
              style={item.kind === "file" ? `--fc: ${FAMILY_COLORS[fileFamily(item.ext)]}` : undefined}
              in:arrive
              animate:flip={{ duration: reduceMotion ? 0 : LEAVE_MS, easing: ease }}
            >
              <button
                class="card-main"
                aria-label={item.kind === "text" ? `Copy ${item.preview}` : `Open ${item.name}${item.missing ? ", Missing" : ""}`}
                aria-disabled={item.missing}
                onpointerdown={(event) => pressCard(event, item)}
                onpointermove={moveCard}
                onpointerup={() => (press = null)}
                onpointercancel={() => (press = null)}
                onclick={() => activateCard(item)}
              >
                <span class="kind" class:badge={item.kind === "file" && item.ext} aria-hidden="true">
                  {#if item.kind === "file" && item.ext}
                    {item.ext}
                  {:else if item.kind === "file"}
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
                </span>
                <span class="copied" class:visible={copied === item.id} aria-live="polite">
                  {#if copied === item.id}
                    <span class="copied-mark">
                      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" /></svg>
                      Copied
                    </span>
                  {/if}
                </span>
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
                <button aria-label="Remove item" title="Remove" onclick={(event) => remove(item, event.currentTarget.closest(".shelf-card"))}>
                  <svg viewBox="0 0 16 16"><path d="m4.5 4.5 7 7 M11.5 4.5l-7 7" /></svg>
                </button>
              </div>
            </div>
          {/each}
        </div>

        {#if shown.length === 0}
          <div class="empty" in:fade={{ duration: 180 }}>
            <span class="sheen" aria-hidden="true"></span>
            <span class="tray-disc" aria-hidden="true">
              <svg class="tray" viewBox="0 0 24 24"><path d="M12 3v11 M7.5 9.5 12 14l4.5-4.5 M4 14v5.5h16V14" /></svg>
            </span>
            {#if mode === "file"}
              <p class="empty-title">Drop files here</p>
              <p class="empty-sub">{files ? "Files stay where they are" : "Accept dropped files is off, turn it on in Settings"}</p>
            {:else}
              <p class="empty-title">Copy text to keep it here</p>
              <p class="empty-sub">{texts ? "Copied text stays in memory" : "Clipboard history is off, turn it on in Settings"}</p>
            {/if}
          </div>
        {/if}
      </div>
    {/key}

    {#if $shelfDrag}
      <div class="drop-fade" aria-hidden="true" in:fade={{ duration: 150 }} out:fade={{ duration: 150 }}>
        <div class="drop">
          <span class="sheen"></span>
        </div>
      </div>
    {/if}
  </div>

  <div class="clear-row">
    <button class="clear" disabled={!canClear || busy} onclick={clearMode}>Clear</button>
  </div>
</div>

<style>
  .defs { position: absolute; }
  /* 146 px box: a 24 px switch, 8 px, an 84 px stage, 8 px and a 20 px clear row. That is 144 px,
     so 1 px sits above and below the block. */
  .shelf-page {
    --accent-a: #8ccbff;
    --accent-b: #6cb8ff;
    --accent-c: #5aaef7;
    width: 100%;
    height: 146px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
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

  /* The mode switch: a track at 8% white with a light-blue pill that slides between the two labels. */
  .switch { position: relative; width: 100%; height: 24px; display: flex; justify-content: center; }
  .tabs {
    position: relative;
    width: 184px;
    height: 24px;
    box-sizing: border-box;
    padding: 2px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    border-radius: 999px;
    background: rgb(255 255 255 / 8%);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 6%);
    transition: opacity 150ms ease;
  }
  .tabs.away { opacity: 0; pointer-events: none; }
  .pill {
    position: absolute;
    top: 2px;
    left: 2px;
    bottom: 2px;
    width: calc(50% - 2px);
    border-radius: 999px;
    /* Lit from above: a lighter top, a bright inner edge, and a soft glow of its own colour. */
    background: linear-gradient(180deg, #b0dcff, var(--accent-b));
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 45%),
      0 2px 10px rgb(108 184 255 / 35%);
    transform: translateX(0);
    transition: transform var(--spring-smooth-ms) var(--spring-smooth);
  }
  .tabs[data-mode="text"] .pill { transform: translateX(100%); }
  .tabs button {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border-radius: 999px;
    background: none;
    font-size: 12px;
    line-height: 16px;
    font-weight: 600;
    color: rgb(255 255 255 / 60%);
    transition: color 150ms ease;
  }
  .tabs button.selected { color: rgb(10 16 24 / 90%); }
  .count { font-size: 11px; font-weight: 600; font-variant-numeric: tabular-nums; opacity: 0.7; }
  .status {
    position: absolute;
    inset: 0;
    margin: 0;
    display: grid;
    place-items: center;
    font-size: 12px;
    line-height: 16px;
    font-weight: 600;
    color: rgb(255 255 255 / 90%);
    white-space: nowrap;
    pointer-events: none;
  }

  /* The stage is the 84 px card row. Both states share it, so the layout never moves. */
  .stage { position: relative; width: 100%; height: 84px; }
  .layer { position: absolute; inset: 0; }
  .shelf-strip { position: absolute; inset: 0; display: flex; gap: 8px; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; scroll-behavior: auto; }
  .shelf-strip::-webkit-scrollbar { display: none; }

  .shelf-card {
    position: relative;
    width: 96px;
    height: 84px;
    flex: 0 0 96px;
    border-radius: 12px;
    /* A glass card lit from above, with its kind's colour glowing in from the top-left corner.
       Text cards take the Shelf blue; files take their family colour (lib/shelf.ts). */
    background:
      radial-gradient(120% 80% at 0% 0%, color-mix(in srgb, var(--fc, #6cb8ff) 20%, transparent), transparent 70%),
      linear-gradient(180deg, rgb(255 255 255 / 11%), rgb(255 255 255 / 6%));
    box-shadow:
      inset 0 0 0 0.5px rgb(255 255 255 / 9%),
      inset 0 1px 0 rgb(255 255 255 / 9%);
    transition:
      opacity 150ms ease,
      transform var(--spring-snappy-ms) var(--spring-snappy);
  }
  /* Hover brightens the card by a white layer's opacity, so no colour transitions. */
  .shelf-card::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 12px;
    background: rgb(255 255 255 / 5%);
    opacity: 0;
    pointer-events: none;
    transition: opacity 150ms ease;
  }
  .shelf-card:hover::after, .shelf-card:focus-within::after { opacity: 1; }
  .shelf-card:hover, .shelf-card:focus-within { transform: translateY(-2px); }
  .shelf-card:has(.card-main:active) { transform: scale(0.97); transition-duration: 150ms, 60ms; }
  .shelf-card.dragging { opacity: 0.45; }
  .shelf-card.missing .card-main { opacity: 0.45; }
  .card-main {
    position: absolute;
    inset: 0;
    padding: 0 10px;
    background: none;
    border-radius: 12px;
    text-align: left;
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }
  .kind, .pin { position: absolute; top: 8px; width: 12px; height: 12px; }
  .kind { left: 10px; color: var(--fc, rgb(255 255 255 / 85%)); }
  /* A file's extension as a small chip in its family colour. */
  .kind.badge {
    width: auto;
    max-width: 52px;
    height: 14px;
    top: 7px;
    padding: 0 5px;
    box-sizing: border-box;
    border-radius: 4px;
    background: color-mix(in srgb, var(--fc) 22%, transparent);
    color: color-mix(in oklab, var(--fc), #fff 30%);
    font-size: 9px;
    line-height: 14px;
    font-weight: 700;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .kind svg, .pin svg { display: block; width: 12px; height: 12px; fill: none; stroke: currentColor; stroke-width: 1.4; stroke-linecap: round; stroke-linejoin: round; }
  .pin { right: 10px; }
  .pin svg { stroke: url(#shelf-aurora-line); }
  .preview, .name { position: absolute; top: 24px; left: 10px; right: 10px; display: -webkit-box; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .preview { -webkit-line-clamp: 2; line-clamp: 2; white-space: pre-wrap; }
  .name { -webkit-line-clamp: 2; line-clamp: 2; font-weight: 600; }
  .foot { position: absolute; left: 10px; right: 10px; bottom: 8px; height: 14px; display: flex; align-items: center; gap: 6px; transition: opacity 150ms ease; }
  .age { color: rgb(255 255 255 / 60%); font-variant-numeric: tabular-nums; }

  .copied {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(10 16 24 / 88%);
    border-radius: 12px;
    font-weight: 600;
    color: var(--accent-a);
    opacity: 0;
    pointer-events: none;
    transition: opacity 150ms ease;
  }
  .copied.visible { opacity: 1; }
  /* The mark pops in on the bouncy spring as the flash appears. */
  .copied-mark {
    display: flex;
    align-items: center;
    gap: 4px;
    animation: pop var(--spring-bouncy-ms) var(--spring-bouncy) both;
  }
  .copied-mark svg { width: 14px; height: 14px; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
  @keyframes pop { from { transform: scale(0.8); } to { transform: none; } }

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
    gap: 2px;
    border-radius: 14px;
    border: 1px dashed rgb(255 255 255 / 18%);
    overflow: hidden;
    text-align: center;
  }
  .empty .sheen { opacity: 0.08; animation-duration: 14s; }
  .tray-disc {
    position: relative;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin-bottom: 3px;
    border-radius: 50%;
    background: radial-gradient(circle at 50% 30%, rgb(140 203 255 / 30%), rgb(108 184 255 / 10%));
    box-shadow: inset 0 0 0 0.5px rgb(140 203 255 / 30%);
  }
  .tray { width: 16px; height: 16px; fill: none; stroke: var(--accent-a); stroke-width: 1.6; stroke-linecap: round; stroke-linejoin: round; }
  .empty-title { position: relative; margin: 0; font-size: 12px; line-height: 16px; font-weight: 600; color: rgb(255 255 255 / 90%); }
  .empty-sub { position: relative; margin: 0; font-size: 11px; line-height: 14px; color: rgb(255 255 255 / 60%); }

  .drop-fade { position: absolute; inset: -3px; pointer-events: none; }
  .drop {
    position: absolute;
    inset: 0;
    border-radius: 15px;
    border: 1.5px dashed rgb(140 203 255 / 70%);
    box-shadow: inset 0 0 24px rgb(108 184 255 / 25%);
    overflow: hidden;
    pointer-events: none;
    animation: pulse 3.5s ease-in-out infinite;
  }
  .drop .sheen { opacity: 0.3; animation-duration: 3.5s; }
  @keyframes pulse { 0%, 100% { opacity: 0.75; } 50% { opacity: 1; } }

  .clear-row { height: 20px; width: 100%; display: flex; align-items: center; justify-content: center; }
  .clear {
    height: 20px;
    padding: 0 10px;
    border-radius: 999px;
    background: none;
    font-size: 11px;
    line-height: 14px;
    font-weight: 600;
    color: rgb(255 255 255 / 60%);
    transition: background-color 150ms ease, color 150ms ease;
  }
  .clear:hover:not(:disabled) { background: rgb(255 255 255 / 8%); color: rgb(255 255 255 / 90%); }
  .clear:disabled { opacity: 0.35; cursor: default; }

  @media (prefers-reduced-motion: reduce) {
    .sheen, .drop { animation: none; }
    .pill { transition: none; }
    .shelf-card, .shelf-card:hover, .shelf-card:focus-within, .shelf-card:has(.card-main:active) { transform: none; }
    .copied-mark { animation: none; }
    .copied, .actions, .foot, .tabs, .tabs button, .clear { transition: none; }
  }
</style>
