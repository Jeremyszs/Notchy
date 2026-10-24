<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { shelf, shelfDrag, shelfConfirmation, refreshShelf } from "./stores/shelf";
  import { config } from "./stores/config";
  import { relativeAge, wheelRoute, type ShelfItem } from "./lib/shelf";

  export let active = false;
  let now = Date.now();
  let copied: number | null = null;
  let flash: ReturnType<typeof setTimeout>;
  $: if (active) void refreshShelf();
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
</script>

<div class="shelf-page">
  <div class="shelf-header">
    <span role="status">{$shelfDrag ? "Drop to shelf" : $shelfConfirmation || "Shelf"}</span>
    <span class="count">{$shelf.length}</span>
    <button class="clear" onclick={() => action("shelf_clear")}>Clear</button>
  </div>
  <!-- This strip owns wheels and pointer swipes, including at either end. -->
  <div class="shelf-strip" class:dropping={$shelfDrag} onwheel={scroll} role="group" aria-label="Shelf items">
    {#each $shelf as item (item.id)}
      <div class="shelf-card" class:missing={item.missing}>
        <button class="card-main" aria-label={item.kind === "text" ? `Copy ${item.preview}` : `Open ${item.name}${item.missing ? ", Missing" : ""}`} aria-disabled={item.missing} onclick={() => activate(item)}>
          {#if item.pinned}<span class="pin-mark" aria-label="Pinned">⌖</span>{/if}
          {#if item.kind === "file"}
            <svg class="file-glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" aria-hidden="true"><path d="M6 3h8l4 4v14H6z M14 3v5h4" /></svg>
            <span class="file-name">{item.name}</span>
            {#if item.ext}<span class="extension">{item.ext}</span>{/if}
          {:else}
            <span class="preview">{item.preview}</span>
          {/if}
          <span class="age">{item.missing ? "Missing" : relativeAge(item.atMs, now)}</span>
          <span class="copied" class:visible={copied === item.id} aria-live="polite">{copied === item.id ? "Copied" : ""}</span>
        </button>
        <div class="card-actions">
          <button aria-label={item.pinned ? "Unpin item" : "Pin item"} title={item.pinned ? "Unpin" : "Pin"} onclick={() => action("shelf_pin", { id: item.id, pinned: !item.pinned })}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m5 2 6 0-1 4 2 3H4l2-3z M8 9v5" /></svg></button>
          {#if item.kind === "file"}<button aria-label="Show in Explorer" title="Show in Explorer" onclick={() => action("shelf_reveal", { id: item.id })}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="M2 4h5l2 2h5v7H2z" /></svg></button>{/if}
          <button aria-label="Remove item" title="Remove" onclick={() => action("shelf_remove", { id: item.id })}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4 4 8 8 M12 4l-8 8" /></svg></button>
        </div>
      </div>
    {:else}
      <p class="shelf-empty">{!$config.shelf.clipboard && $config.shelf.files ? "Drag a file here, or turn on clipboard history in Settings" : "Copy something or drag a file here"}</p>
    {/each}
  </div>
</div>

<style>
  .shelf-page { width: 100%; height: 146px; display: flex; flex-direction: column; justify-content: center; gap: 8px; }
  .shelf-header { height: 16px; flex: 0 0 16px; display: flex; align-items: center; gap: 8px; font-size: 11px; line-height: 16px; color: rgb(255 255 255 / 60%); }
  .count { margin-left: auto; font-variant-numeric: tabular-nums; }
  button { color: inherit; font: inherit; border: 0; cursor: pointer; }
  button:focus-visible { outline: 2px solid rgb(255 255 255 / 70%); outline-offset: -2px; }
  .clear { background: none; padding: 0 2px; }
  .shelf-strip { display: flex; gap: 8px; height: 96px; flex: 0 0 96px; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; scroll-behavior: auto; }
  .shelf-strip::-webkit-scrollbar { display: none; }
  .dropping { outline: 1.5px dashed rgb(255 255 255 / 60%); outline-offset: 3px; border-radius: 12px; }
  .shelf-card { position: relative; width: 88px; height: 96px; flex: 0 0 88px; border-radius: 12px; background: rgb(255 255 255 / 8%); }
  .missing { opacity: 0.45; }
  .card-main { position: relative; width: 100%; height: 100%; padding: 10px 8px 22px; background: none; text-align: left; border-radius: 12px; color: rgb(255 255 255 / 90%); font-size: 11px; line-height: 14px; overflow: hidden; }
  .preview, .file-name { display: -webkit-box; -webkit-box-orient: vertical; overflow: hidden; overflow-wrap: anywhere; }
  .preview { -webkit-line-clamp: 4; line-clamp: 4; white-space: pre-wrap; }
  .file-name { -webkit-line-clamp: 2; line-clamp: 2; }
  .file-glyph { display: block; width: 20px; height: 20px; margin-bottom: 3px; color: rgb(255 255 255 / 85%); }
  .age { position: absolute; bottom: 6px; left: 8px; color: rgb(255 255 255 / 60%); font-size: 10px; }
  .extension { position: absolute; right: 6px; bottom: 6px; max-width: 36px; overflow: hidden; text-overflow: ellipsis; font-size: 9px; line-height: 12px; color: rgb(255 255 255 / 60%); }
  .pin-mark { position: absolute; top: 0; left: 3px; font-size: 11px; }
  .card-actions { display: flex; position: absolute; top: 1px; right: 1px; opacity: 0; pointer-events: none; background: #000; border-radius: 8px; transition: opacity 150ms; }
  .shelf-card:hover .card-actions, .shelf-card:focus-within .card-actions { opacity: 1; pointer-events: auto; }
  .card-actions button { padding: 2px; width: 22px; height: 22px; background: none; border-radius: 6px; color: white; }
  .card-actions svg { width: 16px; height: 16px; fill: none; stroke: currentColor; stroke-width: 1.5; }
  .copied { position: absolute; inset: 0; display: grid; place-items: center; background: #000; opacity: 0; pointer-events: none; transition: opacity 150ms; }
  .copied.visible { opacity: 1; }
  .shelf-empty { margin: auto; font-size: 11px; line-height: 16px; color: rgb(255 255 255 / 55%); text-align: center; white-space: nowrap; letter-spacing: -0.3px; }
  @media (prefers-reduced-motion: reduce) { .copied, .card-actions { transition: none; } }
</style>
