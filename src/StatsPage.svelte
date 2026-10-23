<script lang="ts">
  // The Stats page of the expanded panel: CPU, memory, network and the system drive as four tiles.
  // Each tile has its figure, a detail line and a sparkline of the last 30 readings. The backend
  // reads the system only while this page is open on screen, so the sampler follows the panel.
  import { onDestroy } from "svelte";
  import { islandState } from "./stores/island";
  import { page } from "./stores/pages";
  import { stats, watchStats } from "./stores/stats";
  import {
    diskDetail,
    memoryDetail,
    NETWORK_FLOOR,
    percentFigure,
    rateFigure,
    rateText,
    sparkMax,
    sparkPath,
    type Figure,
  } from "./lib/stats";
  import RollingClock from "./RollingClock.svelte";

  // One restrained accent per tile, drawn on its sparkline only. Text stays white.
  const CPU = "#0A84FF";
  const MEMORY = "#BF5AF2";
  const NETWORK = "#30D158";
  const DISK = "#FF9F0A";

  // The sparkline box, in the same px as the tile's content (the tile is 170 px wide, 10 px inset).
  const SPARK_W = 150;
  const SPARK_H = 14;

  $: watchStats($islandState === "expanded" && $page === "stats");
  onDestroy(() => watchStats(false));

  $: snap = $stats.snapshot;
  $: history = $stats.history;
</script>

{#snippet tile(label: string, figure: Figure, detail: string, color: string, values: number[], max: number)}
  <div class="tile">
    <div class="head">
      <span class="label">{label}</span>
      {#if detail}
        <span class="detail">{detail}</span>
      {/if}
    </div>
    <div class="value">
      {#if figure.number === null}
        <span class="pending">–</span>
      {:else}
        <RollingClock text={figure.number} />
        {#if figure.unit}
          <span class="unit">{figure.unit}</span>
        {/if}
      {/if}
    </div>
    <svg class="spark" viewBox="0 0 {SPARK_W} {SPARK_H}" preserveAspectRatio="none" aria-hidden="true">
      <path d={sparkPath(values, SPARK_W, SPARK_H, max)} style="stroke: {color}" />
    </svg>
  </div>
{/snippet}

<div class="stats" role="group" aria-label="System stats">
  {@render tile("CPU", percentFigure(snap?.cpu ?? null), "", CPU, history.cpu, 100)}
  {@render tile(
    "Memory",
    percentFigure(snap?.memory.percent ?? null),
    snap ? memoryDetail(snap.memory) : "",
    MEMORY,
    history.memory,
    100,
  )}
  {@render tile(
    "Down",
    rateFigure(snap?.network.down ?? null),
    snap ? `Upload ${rateText(snap.network.up)}` : "",
    NETWORK,
    history.down,
    sparkMax(history.down, NETWORK_FLOOR),
  )}
  {@render tile(
    "Disk",
    percentFigure(snap?.disk.percent ?? null),
    snap ? diskDetail(snap.disk) : "",
    DISK,
    history.disk,
    100,
  )}
</div>

<style>
  /* Two rows of two tiles fill the 146 px page box. The 8 px gap between them is the tile gap. */
  .stats {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
    gap: 8px;
    height: 100%;
    color: #fff;
  }

  .tile {
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    min-width: 0;
    padding: 0 10px;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.08);
  }

  .head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 6px;
    min-width: 0;
    font-size: 11px;
    line-height: 14px;
  }

  .label {
    flex: none;
    color: rgba(255, 255, 255, 0.6);
  }

  .detail {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.55);
  }

  /* The figure is a fixed line box, so a long value never changes the tile's height. */
  .value {
    display: flex;
    align-items: baseline;
    gap: 3px;
    min-width: 0;
    height: 26px;
    font-size: 22px;
    line-height: 26px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .unit {
    font-size: 12px;
    line-height: 1;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.6);
  }

  .pending {
    color: rgba(255, 255, 255, 0.55);
  }

  /* The line is drawn at a constant width, and its path changes once per reading. */
  .spark {
    display: block;
    width: 100%;
    height: 14px;
    overflow: visible;
  }

  .spark path {
    fill: none;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
    opacity: 0.9;
  }
</style>
