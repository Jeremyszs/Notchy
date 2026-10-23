<script lang="ts">
  import { onMount } from "svelte";
  import {
    LOCATION_HINT,
    bluetoothDevices,
    joinWifi,
    openSettings,
    wifiNetworks,
    type BtDevice,
    type BtKind,
    type WifiNetwork,
  } from "./stores/controls";

  // The sub-view for one radio: its networks or paired devices. The list is read when the view
  // opens, and again only on Refresh or after a join. The parent unmounts this to close it.
  export let kind: "wifi" | "bluetooth";
  export let onBack: () => void;

  const title = kind === "wifi" ? "Wi-Fi" : "Bluetooth";

  let networks: WifiNetwork[] = [];
  let devices: BtDevice[] = [];
  let loaded = false;
  let busy = false;
  let error: string | null = null;

  // The saved network being joined, and the unsaved one whose hint is showing.
  let joining: string | null = null;
  let hinted: string | null = null;
  let joinError: string | null = null;

  let backButton: HTMLButtonElement | undefined;

  onMount(() => {
    void load();
    backButton?.focus({ preventScroll: true });
  });

  async function load(): Promise<void> {
    busy = true;
    error = null;
    try {
      if (kind === "wifi") networks = await wifiNetworks();
      else devices = await bluetoothDevices();
    } catch (reason) {
      networks = [];
      devices = [];
      error = String(reason);
    } finally {
      busy = false;
      loaded = true;
    }
  }

  // A saved network joins at once. An unsaved one shows what it needs instead.
  async function join(network: WifiNetwork): Promise<void> {
    if (network.connected || joining !== null) return;
    if (!network.known) {
      hinted = hinted === network.ssid ? null : network.ssid;
      return;
    }
    joining = network.ssid;
    joinError = null;
    try {
      await joinWifi(network.ssid);
    } catch (reason) {
      joinError = String(reason);
    } finally {
      joining = null;
    }
    await load();
  }

  function settings(page: "wifi" | "bluetooth" | "location"): void {
    void openSettings(page).catch(() => {});
  }

  // The list scrolls by itself. While it can move, the wheel stays here, so the panel does not
  // turn to the next page. At either end the wheel goes on to the panel.
  function keepWheel(event: WheelEvent): void {
    const list = event.currentTarget as HTMLElement;
    const room =
      event.deltaY > 0
        ? list.scrollTop + list.clientHeight < list.scrollHeight - 1
        : list.scrollTop > 0;
    if (event.deltaY !== 0 && room) event.stopPropagation();
  }

  const LEVELS = [1, 2, 3, 4];
  const SKELETON = [0, 1, 2];

  // Four bars, lit up to the level for the signal. Each level is 25 percent.
  function lit(signal: number, level: number): boolean {
    return level <= Math.ceil(signal / 25);
  }
</script>

{#snippet glyph(id: BtKind)}
  {#if id === "audio"}
    <path d="M4 9h4l5-4v14l-5-4H4z" />
    <path d="M16.5 9a4 4 0 0 1 0 6" />
  {:else if id === "input"}
    <rect x="2.5" y="6.5" width="19" height="11" rx="2" />
    <path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M7 14h10" />
  {:else if id === "phone"}
    <rect x="7" y="2.5" width="10" height="19" rx="2" />
    <path d="M11 18.5h2" />
  {:else}
    <path d="M7 7l10 10-5 5V2l5 5L7 17" />
  {/if}
{/snippet}

<div class="network">
  <header>
    <button
      type="button"
      class="icon"
      aria-label="Back to controls"
      bind:this={backButton}
      onclick={onBack}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
    </button>
    <h2>{title}</h2>
    <button
      type="button"
      class="icon refresh"
      class:busy
      aria-label="Refresh {title}"
      disabled={busy}
      onclick={load}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M20 11a8 8 0 1 0-2.3 5.7M20 4v7h-7" />
      </svg>
    </button>
  </header>

  <div class="body">
    {#if !loaded}
      <ul class="list" aria-label="Loading {title}">
        {#each SKELETON as row (row)}
          <li class="row ghost"><span class="bar"></span></li>
        {/each}
      </ul>
    {:else}
      {#if error}
        <div class="notice" role="alert">
          <p>{error}</p>
          {#if error === LOCATION_HINT}
            <button type="button" class="link" onclick={() => settings("location")}>
              Open settings
            </button>
          {/if}
        </div>
      {/if}

      {#if kind === "wifi"}
        {#if networks.length > 0}
          <ul class="list" onwheel={keepWheel}>
            {#each networks as network (network.ssid)}
              <li>
                <button
                  type="button"
                  class="row"
                  class:current={network.connected}
                  disabled={joining !== null && joining !== network.ssid}
                  onclick={() => join(network)}
                >
                  <svg class="signal" viewBox="0 0 16 12" aria-hidden="true">
                    {#each LEVELS as level (level)}
                      <rect
                        x={(level - 1) * 4.5}
                        y={12 - level * 3}
                        width="2.5"
                        height={level * 3}
                        rx="1"
                        class:dim={!lit(network.signal, level)}
                      />
                    {/each}
                  </svg>
                  <span class="label">{network.ssid}</span>
                  {#if network.secured}
                    <svg class="lock" viewBox="0 0 24 24" aria-label="Secured" role="img">
                      <rect x="5" y="11" width="14" height="10" rx="2" />
                      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
                    </svg>
                  {/if}
                  {#if joining === network.ssid}
                    <span class="spinner" role="status" aria-label="Connecting"></span>
                  {:else if network.connected}
                    <span class="state">Connected</span>
                    <svg class="check" viewBox="0 0 24 24" aria-hidden="true">
                      <path d="M5 12.5l4.5 4.5L19 7.5" />
                    </svg>
                  {/if}
                </button>
                {#if hinted === network.ssid && !network.known && !network.connected}
                  <div class="hint">
                    <span>{network.secured ? "Needs a password" : "Not saved on this PC"}</span>
                    <button type="button" class="link" onclick={() => settings("wifi")}>
                      Open Windows settings
                    </button>
                  </div>
                {/if}
              </li>
            {/each}
          </ul>
        {:else if !error}
          <p class="empty">No networks in range</p>
        {/if}
      {:else if devices.length > 0}
        <ul class="list" onwheel={keepWheel}>
          {#each devices as device (device.id)}
            <li class="row static">
              <svg class="kind" viewBox="0 0 24 24" aria-hidden="true">
                {@render glyph(device.kind)}
              </svg>
              <span class="label">{device.name}</span>
              {#if device.connected}
                <span class="state">Connected</span>
              {/if}
            </li>
          {/each}
        </ul>
      {:else if !error}
        <p class="empty">No paired devices</p>
      {/if}

      {#if joinError}
        <p class="notice" role="alert">{joinError}</p>
      {/if}
    {/if}
  </div>

  <footer>
    <button type="button" class="link" onclick={() => settings(kind)}>
      {title} settings
    </button>
  </footer>
</div>

<style>
  /* Fills the controls page. Header and footer sit on the page box's top and bottom edges, and the
     scrolling list takes the 146 px between them. */
  .network {
    display: flex;
    flex-direction: column;
    height: 100%;
    color: #fff;
  }

  header,
  footer {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    height: 22px;
  }

  footer {
    justify-content: center;
    height: 20px;
  }

  h2 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    line-height: 20px;
  }

  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-height: 0;
  }

  .list {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    list-style: none;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
  }

  .row {
    display: flex;
    box-sizing: border-box;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 6px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: #fff;
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .row:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }

  .row:disabled {
    cursor: default;
    opacity: 0.4;
  }

  .row.static {
    cursor: default;
  }

  .row.static:hover {
    background: none;
  }

  .row.current .label {
    font-weight: 600;
  }

  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .state {
    flex: none;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.55);
  }

  .signal {
    flex: none;
    width: 14px;
    height: 11px;
    fill: currentColor;
    color: rgba(255, 255, 255, 0.9);
  }

  .signal rect.dim {
    opacity: 0.25;
  }

  .lock,
  .check,
  .kind,
  .icon svg {
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .lock {
    width: 10px;
    height: 10px;
    color: rgba(255, 255, 255, 0.55);
  }

  .check {
    width: 12px;
    height: 12px;
  }

  .kind {
    width: 16px;
    height: 16px;
    color: rgba(255, 255, 255, 0.85);
  }

  .icon {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: #fff;
    cursor: pointer;
  }

  .icon svg {
    width: 14px;
    height: 14px;
  }

  .icon:focus-visible,
  .row:focus-visible,
  .link:focus-visible {
    outline: 2px solid rgba(255, 255, 255, 0.7);
    outline-offset: 1px;
  }

  .refresh.busy svg {
    animation: spin 800ms linear infinite;
  }

  .spinner {
    flex: none;
    width: 12px;
    height: 12px;
    border: 1.5px solid rgba(255, 255, 255, 0.25);
    border-top-color: #fff;
    border-radius: 50%;
    animation: spin 800ms linear infinite;
  }

  /* Rows that are not loaded yet. The bar only pulses its opacity. */
  .row.ghost {
    cursor: default;
  }

  .bar {
    display: block;
    width: 70%;
    height: 8px;
    border-radius: 4px;
    background: rgba(255, 255, 255, 0.18);
    animation: pulse 1200ms ease-in-out infinite;
  }

  .hint {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 6px 4px 30px;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.6);
  }

  .notice,
  .empty {
    margin: 4px 6px;
    font-size: 11px;
    line-height: 14px;
    color: rgba(255, 255, 255, 0.6);
  }

  .notice {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .notice p {
    margin: 0;
  }

  /* An empty list centres in the body, between the header and the footer. */
  .empty {
    margin: auto;
    text-align: center;
  }

  /* Text buttons keep a 20px hit area. */
  .link {
    min-height: 20px;
    padding: 0 2px;
    border: 0;
    background: none;
    color: #fff;
    font: inherit;
    font-size: 10px;
    text-decoration: underline;
    text-decoration-color: rgba(255, 255, 255, 0.35);
    text-underline-offset: 2px;
    cursor: pointer;
    white-space: nowrap;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes pulse {
    50% {
      opacity: 0.4;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .refresh.busy svg,
    .spinner {
      animation: pulse 1200ms ease-in-out infinite;
    }

    .bar {
      animation: none;
    }
  }
</style>
