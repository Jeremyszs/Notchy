<script lang="ts">
  // A clock whose changed digits roll, like an odometer. Each digit sits in a fixed column, and the
  // columns are tabular, so the width never moves. A digit that changes rolls out upward while its
  // replacement rises from below. Digits that do not change are not re-keyed, so they stay still.
  import { roll, isDigit } from "./lib/roll";

  export let text: string;

  const reduce =
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches;

  $: chars = text.split("");
</script>

<span class="clock" aria-label={text} role="img">
  {#each chars as ch, i (i)}
    {#if isDigit(ch)}
      <span class="column" aria-hidden="true">
        {#key ch}
          <span class="digit" in:roll={{ dir: 1, reduce }} out:roll={{ dir: -1, reduce }}>{ch}</span>
        {/key}
      </span>
    {:else}
      <span class="sep" aria-hidden="true">{ch}</span>
    {/if}
  {/each}
</span>

<style>
  .clock {
    display: inline-flex;
    align-items: baseline;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  /* A column is a grid, so an outgoing and an incoming digit share one cell and cross while they
     move. The column is as wide as one tabular digit. */
  .column {
    display: inline-grid;
    justify-items: center;
  }

  .digit {
    grid-area: 1 / 1;
    will-change: transform, opacity;
  }

  .sep {
    display: inline-block;
    width: 0.3em;
    text-align: center;
  }
</style>
