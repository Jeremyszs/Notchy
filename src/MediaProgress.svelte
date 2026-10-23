<script lang="ts">
  // The media progress line: the elapsed time, a bar and the length. The elapsed time is extrapolated
  // from the last backend sample while a track plays, and the clock runs only while the panel shows
  // the media page. A click or drag on the bar seeks, and the arrow keys seek by SEEK_STEP_MS. The
  // row stays blank at the same height when the length is unknown.
  import { onDestroy } from "svelte";
  import { mediaState, seekTo } from "./stores/media";
  import { islandState } from "./stores/island";
  import { page } from "./stores/pages";
  import { positionAt } from "./lib/lyrics";
  import {
    canSeek,
    clampPosition,
    formatClock,
    progressRatio,
    seekStep,
    seekTarget,
    SEEK_STEP_MS,
  } from "./lib/progress";

  // How often the elapsed time is redrawn while it runs. The fill moves in these steps.
  const TICK_MS = 250;
  // A seek shows its target until the next backend update. This covers a seek that does nothing.
  const PREVIEW_MS = 2000;

  let now = Date.now();
  let timer: ReturnType<typeof setInterval> | undefined;
  let previewTimer: ReturnType<typeof setTimeout> | undefined;
  // The position a drag or a seek is showing, or null to show the playback position.
  let preview: number | null = null;
  let dragging = false;
  let bar: HTMLElement | undefined;

  $: duration = $mediaState.durationMs;
  $: showing = ($islandState === "expanded" || $islandState === "lyrics") && $page === "media";
  $: setTicking(showing && $mediaState.playing && $mediaState.active && canSeek(duration));
  // The next backend update is the playback position, so it ends any preview.
  $: settle($mediaState.positionAtMs);
  $: live = clampPosition(positionAt($mediaState, now), duration);
  $: shown = preview ?? live;
  $: ratio = progressRatio(shown, duration);

  function setTicking(on: boolean): void {
    if (on === (timer !== undefined)) return;
    if (on) {
      now = Date.now();
      timer = setInterval(() => (now = Date.now()), TICK_MS);
    } else {
      clearInterval(timer);
      timer = undefined;
    }
  }

  function settle(_positionAt: number): void {
    if (dragging) return;
    preview = null;
    clearTimeout(previewTimer);
  }

  function commit(positionMs: number): void {
    preview = positionMs;
    clearTimeout(previewTimer);
    previewTimer = setTimeout(() => (preview = null), PREVIEW_MS);
    seekTo(positionMs).catch(() => {
      // A player that cannot seek is a no-op. The preview ends on its own.
    });
  }

  // The fraction of the bar under the pointer. The bar's own box is the reference, so a drag that
  // leaves it still reads against its edges.
  function ratioAt(event: PointerEvent): number {
    const box = bar?.getBoundingClientRect();
    if (!box || box.width === 0) return 0;
    return (event.clientX - box.left) / box.width;
  }

  function pressBar(event: PointerEvent): void {
    if (!canSeek(duration) || event.button !== 0) return;
    dragging = true;
    bar?.setPointerCapture(event.pointerId);
    preview = seekTarget(ratioAt(event), duration);
  }

  function moveBar(event: PointerEvent): void {
    if (dragging) preview = seekTarget(ratioAt(event), duration);
  }

  function releaseBar(event: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    if (bar?.hasPointerCapture(event.pointerId)) bar.releasePointerCapture(event.pointerId);
    commit(seekTarget(ratioAt(event), duration));
  }

  function cancelBar(event: PointerEvent): void {
    if (!dragging) return;
    dragging = false;
    preview = null;
    if (bar?.hasPointerCapture(event.pointerId)) bar.releasePointerCapture(event.pointerId);
  }

  function keyBar(event: KeyboardEvent): void {
    if (!canSeek(duration)) return;
    const step = event.key === "ArrowRight" ? SEEK_STEP_MS : event.key === "ArrowLeft" ? -SEEK_STEP_MS : 0;
    if (step === 0) return;
    event.preventDefault();
    commit(seekStep(shown, step, duration));
  }

  onDestroy(() => {
    clearInterval(timer);
    clearTimeout(previewTimer);
  });
</script>

{#if canSeek(duration)}
  <div class="progress">
    <span class="time">{formatClock(shown)}</span>
    <div
      bind:this={bar}
      class="bar"
      class:scrub={dragging}
      role="slider"
      tabindex="0"
      aria-label="Playback position"
      aria-valuemin="0"
      aria-valuemax={Math.round(duration / 1000)}
      aria-valuenow={Math.round(shown / 1000)}
      aria-valuetext="{formatClock(shown)} of {formatClock(duration)}"
      onpointerdown={pressBar}
      onpointermove={moveBar}
      onpointerup={releaseBar}
      onpointercancel={cancelBar}
      onkeydown={keyBar}
    >
      <span class="track">
        <span class="fill" style="transform: scaleX({ratio})"></span>
      </span>
    </div>
    <span class="time">{formatClock(duration)}</span>
  </div>
{:else}
  <div class="progress ghost" aria-hidden="true"></div>
{/if}

<style>
  /* One row under the artist, 14 px tall, so it fits beside the cover art without growing the panel.
     The times are tabular, so the bar does not shift as they change. */
  .progress {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: 11px;
    line-height: 14px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.6);
  }

  .ghost {
    height: 14px;
  }

  .time {
    flex: none;
  }

  /* The whole 14 px row is the hit area. The track inside is 3 px. */
  .bar {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 14px;
    cursor: pointer;
  }

  .bar:focus-visible {
    outline: 2px solid rgba(255, 255, 255, 0.7);
    outline-offset: 2px;
  }

  .track {
    position: absolute;
    top: 50%;
    left: 0;
    right: 0;
    height: 3px;
    margin-top: -1.5px;
    border-radius: 1.5px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.16);
  }

  /* The fill scales from its left edge. It eases between ticks, and a drag follows the pointer exactly. */
  .fill {
    position: absolute;
    inset: 0;
    background: #fff;
    transform-origin: left center;
    transition: transform 250ms linear;
  }

  .bar.scrub .fill {
    transition: none;
  }

  @media (prefers-reduced-motion: reduce) {
    .fill {
      transition: none;
    }
  }
</style>
