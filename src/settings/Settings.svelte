<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getVersion } from "@tauri-apps/api/app";
  import { DEFAULT_CONFIG, type Config } from "../stores/config";
  import { PAGE_LABELS, type Page } from "../stores/pages";
  import type { ControlInfo } from "../stores/controls";
  import { FONT_OPTIONS, fontStack, type FontId } from "../lib/font";
  import { accessCopy, isAllowed, setAllowed } from "../lib/notifications";
  import { notificationsStatus } from "../stores/notifications";
  import { batteryStatus } from "../stores/battery";
  import { statusCopy } from "../lib/battery";
  import { moveRow, ordered, type Row } from "./rows";
  import ReorderList from "./ReorderList.svelte";
  import Switch from "./Switch.svelte";

  // The notch's tile row fits seven buttons and toggles. Sliders sit above it and are not counted.
  const MAX_TILES = 7;
  // A burst of clicks or drags saves once, this long after the last change.
  const SAVE_DELAY_MS = 200;
  // Arrow keys move through the font list by one step, in either direction, and wrap.
  const ARROW_STEP: Record<string, number> = {
    ArrowDown: 1,
    ArrowRight: 1,
    ArrowUp: -1,
    ArrowLeft: -1,
  };

  const pageIds = Object.keys(PAGE_LABELS) as Page[];

  let loaded = false;
  let controls: ControlInfo[] = [];
  let pageRows: Row[] = [];
  let controlRows: Row[] = [];
  let lyrics = DEFAULT_CONFIG.lyrics;
  let showHud = DEFAULT_CONFIG.hud;
  let confirmLock = DEFAULT_CONFIG.confirmLock;
  let launchAtLogin = DEFAULT_CONFIG.launchAtLogin;
  let font: FontId = DEFAULT_CONFIG.font;
  let timerSound = DEFAULT_CONFIG.timerSound;
  let idleClock = DEFAULT_CONFIG.idleClock;
  let focusMinutes = DEFAULT_CONFIG.focusMinutes;
  let shortBreakMinutes = DEFAULT_CONFIG.shortBreakMinutes;
  let longBreakMinutes = DEFAULT_CONFIG.longBreakMinutes;
  let notifyOn = DEFAULT_CONFIG.notifications.enabled;
  let deniedApps: string[] = [];
  let batteryOn = DEFAULT_CONFIG.battery.enabled;
  let batteryCharge = DEFAULT_CONFIG.battery.chargeAlerts;
  let batteryLow = DEFAULT_CONFIG.battery.lowBatteryAlerts;
  let confirmingReset = false;
  let status = "";
  let version = "";

  // The settings window shows the chosen font too, so the preview and list follow the choice.
  $: document.documentElement.style.setProperty("--font", fontStack(font));

  $: info = new Map(controls.map((control) => [control.id, control]));
  // Rows as the switches see them: a switch that cannot turn on stays off, and a control
  // this machine cannot run shows why.
  $: pageView = pageLimits(pageRows);
  $: controlView = controlLimits(controlRows, info);

  function buildRows(known: string[], shown: string[], label: (id: string) => string): Row[] {
    return ordered(known, shown).map((id) => ({
      id,
      label: label(id),
      on: shown.includes(id),
      enabled: true,
    }));
  }

  // At least one page stays on.
  function pageLimits(rows: Row[]): Row[] {
    const onCount = rows.filter((row) => row.on).length;
    return rows.map((row) => ({ ...row, enabled: !row.on || onCount > 1 }));
  }

  // Tiles are capped. Sliders are not. Unsupported controls cannot be switched on.
  function controlLimits(rows: Row[], byId: Map<string, ControlInfo>): Row[] {
    const isTile = (id: string) => byId.get(id)?.kind !== "slider";
    const tiles = rows.filter((row) => row.on && isTile(row.id)).length;
    return rows.map((row) => {
      const control = byId.get(row.id);
      const supported = control?.supported ?? true;
      const room = !isTile(row.id) || tiles < MAX_TILES;
      return {
        ...row,
        dim: !supported,
        note: supported ? undefined : (control?.detail ?? "Not available"),
        enabled: row.on || (supported && room),
      };
    });
  }

  function setOn(rows: Row[], id: string, on: boolean): Row[] {
    return rows.map((row) => (row.id === id ? { ...row, on } : row));
  }

  function togglePage(id: string, on: boolean): void {
    pageRows = setOn(pageRows, id, on);
    scheduleSave();
  }

  function movePage(id: string, to: number): void {
    pageRows = moveRow(pageRows, id, to);
    scheduleSave();
  }

  function toggleControl(id: string, on: boolean): void {
    controlRows = setOn(controlRows, id, on);
    scheduleSave();
  }

  function moveControl(id: string, to: number): void {
    controlRows = moveRow(controlRows, id, to);
    scheduleSave();
  }

  function snapshot(): Config {
    return {
      pages: pageRows.filter((row) => row.on).map((row) => row.id),
      controls: controlRows.filter((row) => row.on).map((row) => row.id),
      lyrics,
      confirmLock,
      launchAtLogin,
      font,
      hud: showHud,
      timerSound,
      idleClock,
      focusMinutes,
      shortBreakMinutes,
      longBreakMinutes,
      notifications: { enabled: notifyOn, deniedApps },
      battery: { enabled: batteryOn, chargeAlerts: batteryCharge, lowBatteryAlerts: batteryLow },
    };
  }

  // Turning an app on or off changes only the denied list. The save is the same as any other change.
  function toggleApp(id: string, allowed: boolean): void {
    deniedApps = setAllowed(deniedApps, id, allowed);
    scheduleSave();
  }

  // A typed minute count, kept inside its range. Text that is not a number keeps the old value.
  function readMinutes(input: HTMLInputElement, current: number, min: number, max: number): number {
    const value = Number(input.value);
    if (input.value.trim() === "" || !Number.isFinite(value)) return current;
    return Math.min(max, Math.max(min, Math.round(value)));
  }

  function chooseFont(id: FontId): void {
    font = id;
    scheduleSave();
  }

  // Arrow keys move the choice and its focus together, so the list works from the keyboard alone.
  function fontKeys(event: KeyboardEvent): void {
    const step = ARROW_STEP[event.key];
    if (!step) return;
    event.preventDefault();
    const index = FONT_OPTIONS.findIndex((option) => option.id === font);
    const next = FONT_OPTIONS[(index + step + FONT_OPTIONS.length) % FONT_OPTIONS.length];
    chooseFont(next.id);
    (event.currentTarget as HTMLElement)
      .closest(".font-list")
      ?.querySelector<HTMLElement>(`[data-font="${next.id}"]`)
      ?.focus();
  }

  // Saves run one at a time, each with the settings as they are when it starts, so the last
  // change always wins on disk.
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let queue: Promise<void> = Promise.resolve();

  function scheduleSave(): void {
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      queue = queue.then(save);
    }, SAVE_DELAY_MS);
  }

  async function save(): Promise<void> {
    try {
      await invoke<Config>("config_set", { config: snapshot() });
      status = "";
    } catch (error) {
      status = String(error);
    }
  }

  function apply(settings: Config, list: ControlInfo[]): void {
    const names = new Map(list.map((control) => [control.id, control.label]));
    pageRows = buildRows(pageIds, settings.pages, (id) => PAGE_LABELS[id as Page]);
    controlRows = buildRows(
      list.map((control) => control.id),
      settings.controls,
      (id) => names.get(id) ?? id,
    );
    lyrics = settings.lyrics;
    showHud = settings.hud;
    confirmLock = settings.confirmLock;
    launchAtLogin = settings.launchAtLogin;
    font = settings.font;
    timerSound = settings.timerSound;
    idleClock = settings.idleClock;
    focusMinutes = settings.focusMinutes;
    shortBreakMinutes = settings.shortBreakMinutes;
    longBreakMinutes = settings.longBreakMinutes;
    notifyOn = settings.notifications.enabled;
    deniedApps = settings.notifications.deniedApps;
    batteryOn = settings.battery.enabled;
    batteryCharge = settings.battery.chargeAlerts;
    batteryLow = settings.battery.lowBatteryAlerts;
  }

  function resetDefaults(): void {
    confirmingReset = false;
    apply(DEFAULT_CONFIG, controls);
    scheduleSave();
  }

  // Reloads which controls this machine supports. The order and switches stay as they are.
  async function refreshControls(): Promise<void> {
    try {
      controls = await invoke<ControlInfo[]>("controls_list");
    } catch {
      // Keep the last list. A failed refresh is not worth an error line.
    }
  }

  onMount(() => {
    getVersion().then(
      (value) => (version = value),
      () => {},
    );
    Promise.all([
      invoke<Config>("config_get"),
      invoke<ControlInfo[]>("controls_list"),
    ]).then(
      ([settings, list]) => {
        controls = list;
        apply(settings, list);
        loaded = true;
      },
      (error) => {
        status = `Could not load settings: ${error}`;
      },
    );
  });
</script>

<svelte:window
  onfocus={refreshControls}
  onkeydown={(event) => {
    if (event.key === "Escape") confirmingReset = false;
  }}
/>

<main class="page">
  <header>
    <h1>Notchy</h1>
  </header>

  {#if loaded}
    <section aria-labelledby="pages-title">
      <h2 id="pages-title">Notch pages</h2>
      <ReorderList
        label="Notch pages"
        rows={pageView}
        ontoggle={togglePage}
        onmove={movePage}
      />
    </section>

    <section aria-labelledby="controls-title">
      <div class="heading">
        <h2 id="controls-title">Quick controls</h2>
        <span class="hint">{MAX_TILES} max</span>
      </div>
      <ReorderList
        label="Quick controls"
        rows={controlView}
        ontoggle={toggleControl}
        onmove={moveControl}
      />
    </section>

    <section aria-labelledby="appearance-title">
      <h2 id="appearance-title">Appearance</h2>
      <div class="group">
        <p class="preview">Blinding Lights — The Weeknd</p>
        <span id="font-title" class="font-head">Font</span>
        <div class="font-list" role="group" aria-labelledby="font-title">
          {#each FONT_OPTIONS as option (option.id)}
            {@const chosen = font === option.id}
            <button
              type="button"
              class="font-option"
              data-font={option.id}
              aria-pressed={chosen}
              style="font-family: {option.stack}"
              onclick={() => chooseFont(option.id)}
              onkeydown={fontKeys}
            >
              <span>{option.label}</span>
              {#if chosen}
                <svg viewBox="0 0 24 24" aria-hidden="true">
                  <path d="M5 12.5l4.5 4.5L19 7.5" />
                </svg>
              {/if}
            </button>
          {/each}
        </div>
      </div>
    </section>

    <section aria-labelledby="general-title">
      <h2 id="general-title">General</h2>
      <ul class="group">
        <li class="row">
          <span class="text"><span class="label">Show lyrics</span></span>
          <Switch
            checked={lyrics}
            label="Show lyrics"
            onchange={(on) => {
              lyrics = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Volume and brightness HUD</span></span>
          <Switch
            checked={showHud}
            label="Volume and brightness HUD"
            onchange={(on) => {
              showHud = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Show the time when idle</span></span>
          <Switch
            checked={idleClock}
            label="Show the time when idle"
            onchange={(on) => {
              idleClock = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Confirm before locking</span></span>
          <Switch
            checked={confirmLock}
            label="Confirm before locking"
            onchange={(on) => {
              confirmLock = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Launch at login</span></span>
          <Switch
            checked={launchAtLogin}
            label="Launch at login"
            onchange={(on) => {
              launchAtLogin = on;
              scheduleSave();
            }}
          />
        </li>
      </ul>
    </section>

    <section aria-labelledby="notifications-title">
      <h2 id="notifications-title">Notifications</h2>
      <ul class="group">
        <li class="row">
          <span class="text"><span class="label">Show notifications from other apps</span></span>
          <Switch
            checked={notifyOn}
            label="Show notifications from other apps"
            onchange={(on) => {
              notifyOn = on;
              scheduleSave();
            }}
          />
        </li>
      </ul>
      {#if notifyOn}
        <p class="note" role="status">{accessCopy($notificationsStatus.access)}</p>
      {/if}
      <h3 class="sub">Apps</h3>
      {#if $notificationsStatus.apps.length === 0}
        <ul class="group">
          <li class="empty">Apps appear here after they send a notification.</li>
        </ul>
      {:else}
        <ul class="group">
          {#each $notificationsStatus.apps as app (app.id)}
            <li class="row">
              <span class="text"><span class="label">{app.name}</span></span>
              <Switch
                checked={isAllowed(app.id, deniedApps)}
                label="Show notifications from {app.name}"
                onchange={(on) => toggleApp(app.id, on)}
              />
            </li>
          {/each}
        </ul>
      {/if}
      <p class="note">Notification text is read on this PC and never saved.</p>
    </section>

    <section aria-labelledby="battery-title">
      <h2 id="battery-title">Battery</h2>
      <ul class="group">
        <li class="row">
          <span class="text"><span class="label">Show battery alerts</span></span>
          <Switch
            checked={batteryOn}
            label="Show battery alerts"
            disabled={$batteryStatus === null}
            onchange={(on) => {
              batteryOn = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Charger and full charge</span></span>
          <Switch
            checked={batteryCharge}
            label="Charger and full charge"
            disabled={!batteryOn || !$batteryStatus}
            onchange={(on) => {
              batteryCharge = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Low battery warnings</span></span>
          <Switch
            checked={batteryLow}
            label="Low battery warnings"
            disabled={!batteryOn || !$batteryStatus}
            onchange={(on) => {
              batteryLow = on;
              scheduleSave();
            }}
          />
        </li>
      </ul>
      <p class="note" role="status">{statusCopy($batteryStatus)}</p>
    </section>

    <section aria-labelledby="timer-title">
      <h2 id="timer-title">Timer</h2>
      <ul class="group">
        <li class="row">
          <span class="text"><span class="label">Sound when a timer ends</span></span>
          <Switch
            checked={timerSound}
            label="Sound when a timer ends"
            onchange={(on) => {
              timerSound = on;
              scheduleSave();
            }}
          />
        </li>
        <li class="row">
          <span class="text"><span class="label">Pomodoro focus</span></span>
          <span class="minutes-field">
            <input
              type="number"
              inputmode="numeric"
              min="1"
              max="120"
              value={focusMinutes}
              aria-label="Pomodoro focus length in minutes"
              onchange={(event) => {
                focusMinutes = readMinutes(event.currentTarget, focusMinutes, 1, 120);
                event.currentTarget.value = String(focusMinutes);
                scheduleSave();
              }}
            />
            <span>min</span>
          </span>
        </li>
        <li class="row">
          <span class="text"><span class="label">Short break</span></span>
          <span class="minutes-field">
            <input
              type="number"
              inputmode="numeric"
              min="1"
              max="60"
              value={shortBreakMinutes}
              aria-label="Short break length in minutes"
              onchange={(event) => {
                shortBreakMinutes = readMinutes(event.currentTarget, shortBreakMinutes, 1, 60);
                event.currentTarget.value = String(shortBreakMinutes);
                scheduleSave();
              }}
            />
            <span>min</span>
          </span>
        </li>
        <li class="row">
          <span class="text"><span class="label">Long break, after four focus sessions</span></span>
          <span class="minutes-field">
            <input
              type="number"
              inputmode="numeric"
              min="1"
              max="60"
              value={longBreakMinutes}
              aria-label="Long break length in minutes"
              onchange={(event) => {
                longBreakMinutes = readMinutes(event.currentTarget, longBreakMinutes, 1, 60);
                event.currentTarget.value = String(longBreakMinutes);
                scheduleSave();
              }}
            />
            <span>min</span>
          </span>
        </li>
      </ul>
    </section>

    <div class="reset">
      {#if confirmingReset}
        <p>Reset every setting to its default?</p>
        <button type="button" class="destructive" onclick={resetDefaults}>Reset</button>
        <button type="button" class="plain" onclick={() => (confirmingReset = false)}>
          Cancel
        </button>
      {:else}
        <button type="button" class="plain" onclick={() => (confirmingReset = true)}>
          Reset to defaults
        </button>
      {/if}
    </div>
  {/if}

  {#if status}
    <p class="status" role="alert">{status}</p>
  {/if}

  <footer>
    <p>Notchy {version}</p>
    <p>Lyrics are looked up on lrclib.net.</p>
  </footer>
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: #000;
    color: #fff;
    font-family: var(--font);
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
    user-select: none;
    color-scheme: dark;
  }

  :global(button:focus-visible) {
    outline: 2px solid #0a84ff;
    outline-offset: 2px;
  }

  .page {
    box-sizing: border-box;
    height: 100vh;
    max-width: 640px;
    margin: 0 auto;
    padding: 28px 24px 20px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
  }

  h1 {
    margin: 0 0 20px 4px;
    font-size: 34px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  section {
    margin-bottom: 24px;
  }

  h2 {
    margin: 0 0 6px 16px;
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.55);
  }

  .heading {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    padding-right: 16px;
  }

  .heading h2 {
    margin-bottom: 6px;
  }

  .hint {
    font-size: 13px;
    color: rgba(255, 255, 255, 0.55);
  }

  .group {
    margin: 0;
    padding: 0;
    list-style: none;
    border-radius: 12px;
    background: #111;
    overflow: hidden;
  }

  /* A line in the chosen font, so the choice is visible before it is picked for good. */
  .preview {
    margin: 0;
    padding: 18px 16px 16px;
    font-size: 22px;
    font-weight: 600;
    line-height: 28px;
    letter-spacing: -0.01em;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .font-head {
    display: block;
    padding: 0 16px 6px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.55);
  }

  .font-option {
    display: flex;
    align-items: center;
    justify-content: space-between;
    box-sizing: border-box;
    width: 100%;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: 0;
    background: none;
    color: #fff;
    font-size: 16px;
    text-align: left;
    cursor: pointer;
  }

  .font-option + .font-option {
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .font-option:hover {
    background: rgba(255, 255, 255, 0.06);
  }

  .font-option svg {
    flex: none;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: #0a84ff;
    stroke-width: 2.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    height: 56px;
    padding: 0 16px;
  }

  .row + .row {
    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.08);
  }

  .text {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
  }

  .label {
    font-size: 15px;
    line-height: 20px;
  }

  .minutes-field {
    display: flex;
    flex: none;
    align-items: center;
    gap: 6px;
    font-size: 15px;
    color: rgba(255, 255, 255, 0.55);
  }

  .minutes-field input {
    width: 64px;
    height: 32px;
    box-sizing: border-box;
    padding: 0 8px;
    border: 0;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
    font: inherit;
    font-size: 15px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  .sub {
    margin: 16px 0 6px 16px;
    font-size: 13px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.55);
  }

  .note {
    margin: 8px 16px 0;
    font-size: 13px;
    line-height: 18px;
    color: rgba(255, 255, 255, 0.55);
  }

  .empty {
    box-sizing: border-box;
    min-height: 56px;
    padding: 16px;
    font-size: 15px;
    line-height: 24px;
    color: rgba(255, 255, 255, 0.55);
  }

  .reset {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 44px;
    margin-top: 8px;
  }

  .reset p {
    margin: 0;
    font-size: 14px;
    color: rgba(255, 255, 255, 0.55);
  }

  .plain,
  .destructive {
    padding: 8px 12px;
    border: 0;
    border-radius: 8px;
    background: none;
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }

  .plain {
    color: rgba(255, 255, 255, 0.85);
  }

  .destructive {
    color: #ff453a;
    font-weight: 600;
  }

  .status {
    margin: 12px 4px 0;
    font-size: 13px;
    color: #ff453a;
  }

  footer {
    margin-top: 28px;
    padding: 0 4px;
    font-size: 12px;
    line-height: 18px;
    color: rgba(255, 255, 255, 0.45);
  }

  footer p {
    margin: 0;
  }
</style>
