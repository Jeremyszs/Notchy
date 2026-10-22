<script lang="ts">
  import Island from "./Island.svelte";
  import { config } from "./stores/config";
  import { fontStack } from "./lib/font";

  // The chosen font lives on the root, so every element inherits it. A change in config takes
  // effect at once, and the box sizes stay fixed, so the notch does not move.
  $: document.documentElement.style.setProperty("--font", fontStack($config.font));
</script>

<main>
  <Island />
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
    user-select: none;
  }

  main {
    position: fixed;
    inset: 0;
  }
</style>
