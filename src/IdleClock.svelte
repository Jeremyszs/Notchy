<script lang="ts">
  // The idle pill's time. It re-renders once a minute, from the shared minute clock.
  import { formatHM } from "./lib/clock";
  import { minuteClock } from "./stores/clock";
</script>

<span class="idle-clock">{formatHM($minuteClock)}</span>

<style>
  /* Dim and tabular, so the digits keep their width as the minute changes. */
  .idle-clock {
    font-size: 12px;
    font-weight: 600;
    line-height: 1;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.02em;
    color: rgba(255, 255, 255, 0.55);
  }
</style>
