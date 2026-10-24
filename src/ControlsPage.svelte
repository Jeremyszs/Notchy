<script lang="ts">
  import ControlGlyph from "./ControlGlyph.svelte";
  import { onDestroy } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { SMOOTH_MS, smoothEase } from "./lib/motion";
  import {
    controls,
    visibleControls,
    dragValue,
    endDrag,
    press,
    setValue,
    type ControlInfo,
  } from "./stores/controls";
  import { config } from "./stores/config";
  import { islandState } from "./stores/island";
  import { page } from "./stores/pages";
  import NetworkList from "./NetworkList.svelte";

  // The Wi-Fi and Bluetooth tiles open a sub-view with their lists. Null shows the grid.
  let view: "wifi" | "bluetooth" | null = null;
  // Leaving the panel or the controls page closes the sub-view, so it reopens on the grid.
  $: if ($islandState !== "expanded" || $page !== "controls") view = null;

  // The sub-view slides in 24px with its fade. Reduced motion keeps only the fade.
  const reduceMotion =
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches;

  // The page is 380 wide and 146 tall. Two sliders and seven tiles fit on one page.
  // Anything past that is left out, not wrapped, because a second tile row does not fit.
  const MAX_SLIDERS = 2;
  const MAX_TILES = 7;

  // With confirmLock on, the first tap on Lock arms its tile. Only a second tap within this
  // window locks.
  const ARM_MS = 3000;
  let lockArmed = false;
  let disarm: ReturnType<typeof setTimeout> | undefined;
  onDestroy(() => clearTimeout(disarm));

  function armLock(): void {
    lockArmed = true;
    clearTimeout(disarm);
    disarm = setTimeout(() => (lockArmed = false), ARM_MS);
  }

  $: sliders = $visibleControls
    .filter((c) => c.kind === "slider")
    .slice(0, MAX_SLIDERS);
  $: tiles = $visibleControls
    .filter((c) => c.kind !== "slider")
    .slice(0, MAX_TILES);

  // Toggles are on at 1. A button has no state, so it only fires.
  function isOn(c: ControlInfo): boolean {
    return c.value > 0.5;
  }

  function activate(c: ControlInfo): void {
    if (!c.supported) return;
    if (c.kind !== "button") {
      void setValue(c.id, isOn(c) ? 0 : 1);
      return;
    }
    if (c.id === "lock" && $config.confirmLock && !lockArmed) {
      armLock();
      return;
    }
    if (c.id === "lock") {
      lockArmed = false;
      clearTimeout(disarm);
    }
    void press(c.id);
  }

  function reason(c: ControlInfo): string | undefined {
    return c.supported ? undefined : (c.detail ?? "Not available");
  }

  function sliderValue(event: Event): number {
    return Number((event.currentTarget as HTMLInputElement).value);
  }

  // The thumb is 16px wide, so the fill edge sits at 8px + the fraction of the 16px-shorter track.
  function fill(value: number): string {
    return `calc(8px + (100% - 16px) * ${value / 100})`;
  }
</script>


<div class="controls-page">
  <div class="main" class:away={view !== null} inert={view !== null}>
    {#if $controls === null}
      <!-- Waiting for the first list. -->
    {:else if $visibleControls.length === 0}
      <p class="empty">Controls unavailable</p>
    {:else}
      {#if sliders.length > 0}
        <div class="sliders">
          {#each sliders as c (c.id)}
            <div class="slider" class:off={!c.supported} title={reason(c)}>
              <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true">
                <ControlGlyph id={c.id} />
              </svg>
              <input
                type="range"
                min="0"
                max="100"
                step="1"
                value={c.value}
                aria-label={c.label}
                disabled={!c.supported}
                style="--p: {fill(c.value)}"
                oninput={(event) => dragValue(c.id, sliderValue(event))}
                onchange={(event) => endDrag(c.id, sliderValue(event))}
              />
            </div>
          {/each}
        </div>
      {/if}

      {#if tiles.length > 0}
        <div class="tiles">
          {#each tiles as c (c.id)}
            <div
              class="tile"
              class:off={!c.supported}
              class:armed={c.id === "lock" && lockArmed}
              title={reason(c)}
            >
              <button
                type="button"
                class="round"
                class:on={c.kind === "toggle" && isOn(c)}
                role={c.kind === "toggle" ? "switch" : undefined}
                aria-checked={c.kind === "toggle" ? isOn(c) : undefined}
                aria-label={c.id === "lock" && lockArmed ? "Tap again to lock" : c.label}
                aria-disabled={!c.supported}
                onclick={() => activate(c)}
              >
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <ControlGlyph id={c.id} />
                </svg>
              </button>
              {#if c.id === "wifi" || c.id === "bluetooth"}
                <button
                  type="button"
                  class="more"
                  aria-label={c.id === "wifi" ? "Show Wi-Fi networks" : "Show Bluetooth devices"}
                  onclick={() => (view = c.id === "wifi" ? "wifi" : "bluetooth")}
                >
                  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
                </button>
              {/if}
              <span class="name">{c.id === "lock" && lockArmed ? "Tap again" : c.label}</span>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  {#if view}
    <div
      class="sheet"
      in:fly={{ x: reduceMotion ? 0 : 24, duration: SMOOTH_MS, easing: smoothEase }}
      out:fade={{ duration: 160 }}
    >
      <NetworkList kind={view} onBack={() => (view = null)} />
    </div>
  {/if}
</div>

<style>
  .controls-page {
    position: relative;
    height: 100%;
    color: #fff;
  }

  /* The grid fades out while a sub-view is open. Inert keeps it out of focus and taps. The sliders
     and the tiles are centred as one block, with 12 px between them. */
  .main {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 12px;
    height: 100%;
    transition: opacity 160ms ease;
  }

  .main.away {
    opacity: 0;
  }

  .sheet {
    position: absolute;
    inset: 0;
  }

  .empty {
    margin: auto;
    color: rgba(255, 255, 255, 0.55);
  }

  .sliders {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .slider {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 28px;
  }

  .slider.off,
  .tile.off {
    opacity: 0.4;
  }

  .glyph,
  .round svg {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .glyph {
    width: 18px;
    height: 18px;
    color: rgba(255, 255, 255, 0.85);
  }

  /* A rounded 8px track. The fill is a hard gradient stop driven by --p, so no property animates.
     The thumb is transparent: it only sets the drag geometry. */
  input[type="range"] {
    -webkit-appearance: none;
    appearance: none;
    flex: 1;
    min-width: 0;
    height: 28px;
    margin: 0;
    background: transparent;
    cursor: pointer;
  }

  input[type="range"]:focus {
    outline: none;
  }

  input[type="range"]::-webkit-slider-runnable-track {
    height: 8px;
    border-radius: 4px;
    background: linear-gradient(
      to right,
      #fff var(--p),
      rgba(255, 255, 255, 0.18) var(--p)
    );
  }

  input[type="range"]:focus-visible::-webkit-slider-runnable-track {
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.7);
  }

  input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -4px;
    border: 0;
    background: transparent;
  }

  input[type="range"]:disabled {
    cursor: default;
    pointer-events: none;
  }

  .tiles {
    display: flex;
    justify-content: space-between;
  }

  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    width: 48px;
  }

  /* A chevron on the tile's top right opens the sub-view. The 20px button is the hit area, and the
     black disc inside it is drawn over the round's edge. */
  .more {
    position: absolute;
    top: 0;
    right: 0;
    z-index: 1;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    background: none;
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .more::before {
    content: "";
    position: absolute;
    inset: 2px;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 50%;
    background: #000;
  }

  .more svg {
    position: relative;
    width: 9px;
    height: 9px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .more:focus-visible {
    outline: 2px solid rgba(255, 255, 255, 0.7);
    outline-offset: 1px;
    border-radius: 50%;
  }

  .round {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.12);
    color: #fff;
    cursor: pointer;
    transition:
      transform 120ms ease,
      opacity 120ms ease;
  }

  .round.on {
    background: #fff;
    color: #000;
  }

  .round svg {
    width: 20px;
    height: 20px;
  }

  /* Press feedback for every supported tile. Unsupported tiles do not respond. */
  .round[aria-disabled="false"]:active {
    transform: scale(0.94);
    opacity: 0.7;
  }

  /* An armed lock tile lifts and fades its ring in, using transform and opacity only. */
  .tile.armed .round {
    transform: scale(1.1);
    opacity: 1;
    outline: 2px solid rgba(255, 255, 255, 0.85);
    outline-offset: 2px;
  }

  .round:focus-visible {
    outline: 2px solid rgba(255, 255, 255, 0.7);
    outline-offset: 2px;
  }

  .name {
    max-width: 100%;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 10px;
    line-height: 12px;
    color: rgba(255, 255, 255, 0.6);
  }

  @media (prefers-reduced-motion: reduce) {
    .round {
      transition: opacity 120ms ease;
    }

    .round[aria-disabled="false"]:active {
      transform: none;
    }
  }
</style>
