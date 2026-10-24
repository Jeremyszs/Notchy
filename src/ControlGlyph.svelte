<svelte:options namespace="svg" />

<script lang="ts">
  // The quick-control icons, drawn on a 24 px grid. Render inside an <svg viewBox="0 0 24 24">.
  // Shared by the notch's Controls page and the Settings layout editor.
  export let id: string;
</script>

{#if id === "volume"}
  <path d="M4 9v6h4l5 4V5L8 9H4z" />
  <path d="M16.5 8.5a5 5 0 0 1 0 7M19 6a8.5 8.5 0 0 1 0 12" />
{:else if id === "mute"}
  <path d="M4 9v6h4l5 4V5L8 9H4z" />
  <path d="M17 9l5 6M22 9l-5 6" />
{:else if id === "brightness"}
  <circle cx="12" cy="12" r="4" />
  <path
    d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"
  />
{:else if id === "wifi"}
  <path d="M2 9a15 15 0 0 1 20 0M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0" />
  <path d="M12 19.5h.01" />
{:else if id === "bluetooth"}
  <path d="M7 7l10 10-5 5V2l5 5L7 17" />
{:else if id === "airplane"}
  <path d="M3 13l7-1 4-7h2l-2 7 5-1 2-3h2l-1.5 4L21 14v2l-3-.5L16 19h-2l2-4-5 1-3 3H6l1-4-3-1z" />
{:else if id === "awake"}
  <path d="M2 12s3.5-6 10-6 10 6 10 6-3.5 6-10 6S2 12 2 12z" />
  <circle cx="12" cy="12" r="3" />
{:else if id === "darkmode"}
  <path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z" />
{:else if id === "lock"}
  <rect x="5" y="11" width="14" height="10" rx="2" />
  <path d="M8 11V7a4 4 0 0 1 8 0v4" />
{:else if id === "screenshot"}
  <path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2" />
  <circle cx="12" cy="12" r="3" />
{:else if id === "mic"}
  <path d="M12 3a3 3 0 0 0-3 3v6a3 3 0 0 0 6 0V6a3 3 0 0 0-3-3z" />
  <path d="M19 11a7 7 0 0 1-14 0M12 18v3" />
{:else}
  <circle cx="12" cy="12" r="8" />
{/if}
