<script lang="ts">
  // Settings icons on a 24 px grid, stroked in the current colour. Pages share their ids with the
  // notch pager (stores/pages.ts), so a page chip and its nav tile draw the same picture.
  let { name, size = 18 }: { name: string; size?: number } = $props();
</script>

<svg class="glyph" viewBox="0 0 24 24" width={size} height={size} aria-hidden="true">
  {#if name === "features"}
    <path d="M12 3l1.8 4.6L18.5 9l-4.7 1.5L12 15l-1.8-4.5L5.5 9l4.7-1.4z" />
    <path d="M18.5 15l.8 2 2 .8-2 .7-.8 2-.7-2-2-.7 2-.8z" />
  {:else if name === "layout"}
    <rect x="3" y="4" width="18" height="16" rx="3" />
    <path d="M3 9h18M9 9v11" />
  {:else if name === "appearance"}
    <path d="M4 19 10 5h1l6 14M6.5 14h8" />
    <path d="M17 19v-6.5M17 12.5a2.5 2.5 0 1 1 3.5 2.3" />
  {:else if name === "media"}
    <path d="M9 18V5l11-2v13" />
    <circle cx="6.5" cy="18" r="2.5" />
    <circle cx="17.5" cy="16" r="2.5" />
  {:else if name === "controls"}
    <path d="M4 7h10M18 7h2M4 17h4M12 17h8" />
    <circle cx="16" cy="7" r="2" />
    <circle cx="10" cy="17" r="2" />
  {:else if name === "timer"}
    <circle cx="12" cy="13.5" r="7.5" />
    <path d="M12 13.5V9.5M9.5 2.5h5M18.5 6.5l1.5-1.5" />
  {:else if name === "stats"}
    <path d="M3 17l5-6 4 3 4-7 5 6" />
    <path d="M3 21h18" />
  {:else if name === "calendar"}
    <rect x="3.5" y="5" width="17" height="15.5" rx="3" />
    <path d="M3.5 10h17M8 3v4M16 3v4" />
  {:else if name === "shelf"}
    <path d="M4 13.5 6.5 5h11l2.5 8.5V19H4z" />
    <path d="M4 13.5h5a3 3 0 0 0 6 0h5" />
  {:else if name === "notifications"}
    <path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z" />
    <path d="M10 20.5a2 2 0 0 0 4 0" />
  {:else if name === "battery"}
    <rect x="2.5" y="7" width="17" height="10" rx="2.5" />
    <path d="M21.5 10.5v3" />
    <path d="M10.5 9 8.5 12h3l-2 3" />
  {:else if name === "hotkeys"}
    <rect x="2.5" y="6" width="19" height="12" rx="2.5" />
    <path d="M6.5 10h.01M10 10h.01M13.5 10h.01M17 10h.01M8 14h8" />
  {:else if name === "general"}
    <circle cx="12" cy="12" r="3" />
    <path d="M12 2.5v3M12 18.5v3M4.6 4.6l2.1 2.1M17.3 17.3l2.1 2.1M2.5 12h3M18.5 12h3M4.6 19.4l2.1-2.1M17.3 6.7l2.1-2.1" />
  {:else if name === "lyrics"}
    <path d="M4 7h11M4 12h16M4 17h8" />
  {:else if name === "hud"}
    <path d="M3 9.5v5h3.5L11 18V6L6.5 9.5z" />
    <path d="M15 12h6" />
  {:else if name === "clock"}
    <circle cx="12" cy="12" r="8.5" />
    <path d="M12 7.5V12l3 2" />
  {:else if name === "lock"}
    <rect x="5" y="11" width="14" height="10" rx="2" />
    <path d="M8 11V7a4 4 0 0 1 8 0v4" />
  {:else if name === "power"}
    <path d="M12 3v8" />
    <path d="M7 6.5a7.5 7.5 0 1 0 10 0" />
  {:else if name === "bolt"}
    <path d="M13 2 5 13.5h6L10 22l9-12h-6z" />
  {:else if name === "low"}
    <rect x="2.5" y="7" width="17" height="10" rx="2.5" />
    <path d="M21.5 10.5v3M6 10v4" />
  {:else if name === "clipboard"}
    <rect x="5" y="4.5" width="14" height="17" rx="2.5" />
    <path d="M9 4.5V3h6v1.5M8.5 11h7M8.5 15h5" />
  {:else if name === "tray"}
    <path d="M12 3v11M7.5 9.5 12 14l4.5-4.5M4 14v5.5h16V14" />
  {:else if name === "sound"}
    <path d="M3 9.5v5h3.5L11 18V6L6.5 9.5z" />
    <path d="M15 9a4 4 0 0 1 0 6M17.5 6.5a7.5 7.5 0 0 1 0 11" />
  {:else if name === "plus"}
    <path d="M12 6v12M6 12h12" />
  {:else if name === "minus"}
    <path d="M6 12h12" />
  {:else if name === "chevron"}
    <path d="m9 6 6 6-6 6" />
  {:else if name === "reset"}
    <path d="M4 12a8 8 0 1 0 2.4-5.7" />
    <path d="M4 4v4.5h4.5" />
  {:else}
    <circle cx="12" cy="12" r="8" />
  {/if}
</svg>

<style>
  .glyph {
    display: block;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
