<script lang="ts">
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getVersion } from "@tauri-apps/api/app";
  import { DEFAULT_CONFIG, type Config, type HotkeySettings } from "../stores/config";
  import { hotkeysStatus } from "../stores/hotkeys";
  import { bindingError, cleanAccelerator, HOTKEY_ACTIONS, type HotkeyAction } from "../lib/hotkeys";
  import { PAGE_LABELS, type Page } from "../stores/pages";
  import type { ControlInfo } from "../stores/controls";
  import { FONT_OPTIONS, fontStack, type FontId } from "../lib/font";
  import { accessCopy, isAllowed, setAllowed } from "../lib/notifications";
  import { notificationsStatus } from "../stores/notifications";
  import { batteryStatus } from "../stores/battery";
  import { statusCopy } from "../lib/battery";
  import { hourLabel, LEAD_OPTIONS, leadLabel } from "../lib/calendar";
  import { rise } from "../lib/motion";
  import { arrange, ordered, showRow, type Row } from "./rows";
  import CalendarMonth from "./CalendarMonth.svelte";
  import ControlGlyph from "../ControlGlyph.svelte";
  import Glyph from "./Glyph.svelte";
  import Strip from "./Strip.svelte";
  import Switch from "./Switch.svelte";

  // The notch's Controls page fits two sliders above seven buttons and toggles.
  const MAX_TILES = 7;
  const MAX_SLIDERS = 2;
  // A burst of clicks or drags saves once, this long after the last change.
  const SAVE_DELAY_MS = 200;
  // Arrow keys move through the font list by one step, in either direction, and wrap.
  const ARROW_STEP: Record<string, number> = {
    ArrowDown: 1,
    ArrowRight: 1,
    ArrowUp: -1,
    ArrowLeft: -1,
  };

  // The sections in the sidebar. Each has a tile colour, as System Settings does.
  type SectionId =
    | "features"
    | "layout"
    | "appearance"
    | "timer"
    | "calendar"
    | "notifications"
    | "battery"
    | "shelf"
    | "hotkeys"
    | "general";
  const SECTIONS: { id: SectionId; label: string; color: string; blurb: string }[] = [
    { id: "features", label: "Features", color: "#0A84FF", blurb: "Turn the parts of the notch on or off." },
    { id: "layout", label: "Notch layout", color: "#5E5CE6", blurb: "Choose the pages you swipe through and the buttons on the Controls page." },
    { id: "appearance", label: "Appearance", color: "#BF5AF2", blurb: "The font the notch uses." },
    { id: "timer", label: "Timer", color: "#FF9F0A", blurb: "Pomodoro lengths and the finish sound." },
    { id: "calendar", label: "Calendar", color: "#FF6A3D", blurb: "Event reminders and your notes." },
    { id: "notifications", label: "Notifications", color: "#FF453A", blurb: "Show other apps' notifications in the notch." },
    { id: "battery", label: "Battery", color: "#30D158", blurb: "Charger and low battery alerts." },
    { id: "shelf", label: "Shelf", color: "#6CB8FF", blurb: "Keep copied text and dropped files at hand." },
    { id: "hotkeys", label: "Hotkeys", color: "#8E8E93", blurb: "Keyboard shortcuts that work in every app." },
    { id: "general", label: "General", color: "#636366", blurb: "Startup, locking and reset." },
  ];
  const SECTION_KEY = "notchy.settings.section";
  const PAGE_COLORS: Record<Page, string> = {
    media: "#FF375F",
    controls: "#8E8E93",
    timer: "#FF9F0A",
    stats: "#0A84FF",
    calendar: "#FF6A3D",
    shelf: "#6CB8FF",
  };

  const pageIds = Object.keys(PAGE_LABELS) as Page[];

  let section: SectionId = "features";
  try {
    const kept = localStorage.getItem(SECTION_KEY);
    if (SECTIONS.some((entry) => entry.id === kept)) section = kept as SectionId;
  } catch {
    // Storage can be blocked. The first section is fine.
  }
  $: current = SECTIONS.find((entry) => entry.id === section) ?? SECTIONS[0];
  let content: HTMLElement;

  function open(id: SectionId): void {
    section = id;
    content?.scrollTo({ top: 0 });
    try {
      localStorage.setItem(SECTION_KEY, id);
    } catch {
      // Remembering the section is a convenience only.
    }
  }

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
  let statsOn = DEFAULT_CONFIG.stats.enabled;
  let shelfClipboard = DEFAULT_CONFIG.shelf.clipboard;
  let shelfFiles = DEFAULT_CONFIG.shelf.files;
  let calendarOn = DEFAULT_CONFIG.calendar.enabled;
  let calendarLead = DEFAULT_CONFIG.calendar.leadMinutes;
  let calendarHour = DEFAULT_CONFIG.calendar.allDayHour;
  let hotkeys: HotkeySettings = { ...DEFAULT_CONFIG.hotkeys };
  const HOURS = Array.from({ length: 24 }, (_, hour) => hour);
  let confirmingReset = false;
  let status = "";
  let version = "";
  // Read out after a move or a change in the layout editor.
  let announcement = "";

  // The settings window shows the chosen font too, so the preview and list follow the choice.
  $: document.documentElement.style.setProperty("--font", fontStack(font));

  $: info = new Map(controls.map((control) => [control.id, control]));

  // --- Notch layout: pages
  $: shownPages = pageRows.filter((row) => row.on).map((row) => row.id);
  $: hiddenPages = pageRows.filter((row) => !row.on);
  // A page whose feature is off is still in the order, but the notch skips it. Its chip says so.
  $: pageOff = (id: string) => (id === "stats" && !statsOn) || (id === "shelf" && !shelfClipboard && !shelfFiles);

  function arrangePages(ids: string[]): void {
    pageRows = arrange(pageRows, ids);
    scheduleSave();
  }

  function showPage(id: string, on: boolean): void {
    pageRows = showRow(pageRows, id, on);
    announcement = `${PAGE_LABELS[id as Page]} ${on ? "added" : "removed"}`;
    scheduleSave();
  }

  // --- Notch layout: quick controls. Sliders sit above the buttons in the notch, so each has its row.
  $: isSlider = (id: string) => info.get(id)?.kind === "slider";
  $: shownSliders = controlRows.filter((row) => row.on && isSlider(row.id)).map((row) => row.id);
  $: shownTiles = controlRows.filter((row) => row.on && !isSlider(row.id)).map((row) => row.id);
  $: hiddenControls = controlRows.filter((row) => !row.on);
  $: controlLabel = (id: string) => info.get(id)?.label ?? id;

  function arrangeControls(sliders: string[], tiles: string[]): void {
    controlRows = arrange(controlRows, [...sliders, ...tiles]);
    scheduleSave();
  }

  // Why a hidden control cannot be added right now, or null when it can.
  function controlBlock(id: string, sliders: number, tiles: number): string | null {
    const control = info.get(id);
    if (control && !control.supported) return control.detail ?? "Not available on this PC";
    if (isSlider(id) && sliders >= MAX_SLIDERS) return `${MAX_SLIDERS} sliders at most`;
    if (!isSlider(id) && tiles >= MAX_TILES) return `${MAX_TILES} buttons at most`;
    return null;
  }

  function showControl(id: string, on: boolean): void {
    controlRows = showRow(controlRows, id, on);
    announcement = `${controlLabel(id)} ${on ? "added" : "removed"}`;
    scheduleSave();
  }

  function buildRows(known: string[], shown: string[], label: (id: string) => string): Row[] {
    return ordered(known, shown).map((id) => ({
      id,
      label: label(id),
      on: shown.includes(id),
      enabled: true,
    }));
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
      stats: { enabled: statsOn },
      shelf: { clipboard: shelfClipboard, files: shelfFiles },
      calendar: { enabled: calendarOn, leadMinutes: calendarLead, allDayHour: calendarHour },
      hotkeys,
    };
  }

  // A shortcut is saved tidied, so "ctrl + t" is kept as "ctrl+t". An empty one leaves its action unbound.
  function setHotkey(action: HotkeyAction, typed: string): void {
    hotkeys = { ...hotkeys, [action]: cleanAccelerator(typed) };
    scheduleSave();
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

  type Length = "focus" | "short" | "long";
  const LENGTHS: { id: Length; label: string; color: string; max: number }[] = [
    { id: "focus", label: "Focus", color: "#FF453A", max: 120 },
    { id: "short", label: "Short break", color: "#30D158", max: 60 },
    { id: "long", label: "Long break", color: "#64D2FF", max: 60 },
  ];
  $: lengthOf = (id: Length) => (id === "focus" ? focusMinutes : id === "short" ? shortBreakMinutes : longBreakMinutes);

  function setLength(id: Length, value: number): void {
    const max = LENGTHS.find((entry) => entry.id === id)?.max ?? 60;
    const next = Math.min(max, Math.max(1, Math.round(value)));
    if (id === "focus") focusMinutes = next;
    else if (id === "short") shortBreakMinutes = next;
    else longBreakMinutes = next;
    scheduleSave();
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
      .closest(".font-grid")
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
    statsOn = settings.stats.enabled;
    shelfClipboard = settings.shelf.clipboard;
    shelfFiles = settings.shelf.files;
    calendarOn = settings.calendar.enabled;
    calendarLead = settings.calendar.leadMinutes;
    calendarHour = settings.calendar.allDayHour;
    hotkeys = { ...settings.hotkeys };
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

  // The island's "Add note" asks for the Calendar section with the add form open on a day. An open
  // window hears the event; a window built for the request takes the day once it loads. Either way the
  // pending day is taken, so it cannot open the form again later.
  let addRequest: { day: string; seq: number } | null = null;

  function requestAdd(day: string): void {
    addRequest = { day, seq: (addRequest?.seq ?? 0) + 1 };
    open("calendar");
  }

  function takePendingAdd(): void {
    invoke<string | null>("calendar_take_add_date").then(
      (day) => {
        if (day) requestAdd(day);
      },
      () => {},
    );
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    let gone = false;
    listen<string>("calendar://add-note", () => takePendingAdd()).then((off) => {
      if (gone) off();
      else stop = off;
    });
    takePendingAdd();
    return () => {
      gone = true;
      stop?.();
    };
  });

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

  // Section content arrives with the shared rise, so a section change reads as one motion.
  const reduceMotion =
    typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  const sectionIn = { y: 8, blur: 0, reduced: reduceMotion };
</script>

<svelte:window
  onfocus={refreshControls}
  onkeydown={(event) => {
    if (event.key === "Escape") confirmingReset = false;
  }}
/>

<!-- A feature card: a small picture of the notch doing the thing, a name, one line and a switch.
     The card's link opens the section with that feature's options. -->
{#snippet feature(
  name: string,
  line: string,
  color: string,
  checked: boolean,
  onchange: (on: boolean) => void,
  art: import("svelte").Snippet,
  more: SectionId | null = null,
  disabled = false,
)}
  <article class="feature" class:off={!checked} style="--c: {color}">
    <div class="art" aria-hidden="true">{@render art()}</div>
    <div class="feature-body">
      <div class="feature-text">
        <h3>{name}</h3>
        <p>{line}</p>
      </div>
      <Switch {checked} label={name} {disabled} {onchange} />
    </div>
    {#if more}
      <button type="button" class="more" onclick={() => open(more)}>
        Options <Glyph name="chevron" size={12} />
      </button>
    {/if}
  </article>
{/snippet}

<!-- The little notch pictures. Each is drawn with plain boxes on a soft wallpaper glow. -->
{#snippet artLyrics()}
  <div class="mock panel">
    <span class="m-art"></span>
    <span class="m-lines"><i class="dim" style="width:46px"></i><i style="width:62px"></i><i class="dim" style="width:38px"></i></span>
  </div>
{/snippet}
{#snippet artHud()}
  <div class="mock pill wide">
    <Glyph name="hud" size={10} />
    <span class="m-bar"><i style="width:62%; background: linear-gradient(90deg, #7dd3fc, #fff)"></i></span>
  </div>
{/snippet}
{#snippet artClock()}
  <div class="mock pill small"><span class="m-text">9:41</span></div>
{/snippet}
{#snippet artNotice()}
  <div class="mock pill wide">
    <span class="m-dot" style="background: linear-gradient(135deg, #0a84ff, #64d2ff)">M</span>
    <span class="m-lines row"><i style="width:30px"></i><i class="dim" style="width:40px"></i></span>
  </div>
{/snippet}
{#snippet artBattery()}
  <div class="mock pill wide">
    <span class="m-batt"><i style="width:80%"></i></span>
    <span class="m-text green">80%</span>
  </div>
{/snippet}
{#snippet artStats()}
  <div class="mock panel">
    <svg class="m-spark" viewBox="0 0 80 28" aria-hidden="true">
      <path d="M0 22 10 18 20 20 30 9 40 13 50 6 60 12 70 4 80 9" stroke="#0a84ff" />
      <path d="M0 25 10 23 20 24 30 19 40 21 50 17 60 20 70 15 80 18" stroke="#bf5af2" />
    </svg>
  </div>
{/snippet}
{#snippet artCalendar()}
  <div class="mock pill wide">
    <span style="color: #ff6a3d"><Glyph name="calendar" size={10} /></span>
    <span class="m-lines row"><i style="width:34px"></i><i class="dim" style="width:22px"></i></span>
  </div>
{/snippet}
{#snippet artTimer()}
  <div class="mock pill wide">
    <span class="m-ring"></span>
    <span class="m-text">05:00</span>
  </div>
{/snippet}
{#snippet artShelf()}
  <div class="mock panel shelf">
    <i style="--k: #ff453a"></i><i style="--k: #30d158"></i><i style="--k: #6cb8ff"></i>
  </div>
{/snippet}
{#snippet artKeys()}
  <div class="keys"><kbd>Ctrl</kbd><kbd>Alt</kbd><kbd>Space</kbd></div>
{/snippet}

<!-- A settings row: an icon tile, a name with an optional line under it, and a control on the right. -->
{#snippet rowHead(icon: string, color: string, name: string, line: string = "")}
  <span class="tile" style="--c: {color}"><Glyph name={icon} size={16} /></span>
  <span class="text">
    <span class="label">{name}</span>
    {#if line}<span class="sub">{line}</span>{/if}
  </span>
{/snippet}

{#snippet pageChip(id: string, index = -1)}
  {@const off = pageOff(id)}
  <span class="page-chip" class:muted={off} style="--c: {PAGE_COLORS[id as Page]}" title={off ? "This feature is off, so the notch skips it" : undefined}>
    <span class="chip-icon"><Glyph name={id} size={16} /></span>
    <span class="chip-label">{PAGE_LABELS[id as Page]}</span>
    {#if off}<span class="chip-note">Off</span>{/if}
    {#if index >= 0}<span class="chip-order" aria-hidden="true">{index + 1}</span>{/if}
  </span>
{/snippet}

{#snippet tileChip(id: string, _index = -1)}
  {@const missing = info.get(id)?.supported === false}
  <span class="tile-chip" class:muted={missing} title={missing ? (info.get(id)?.detail ?? "Not available on this PC") : undefined}>
    <span class="round"><svg viewBox="0 0 24 24" aria-hidden="true"><ControlGlyph {id} /></svg></span>
    <span class="chip-label">{controlLabel(id)}</span>
  </span>
{/snippet}

{#snippet sliderChip(id: string, _index = -1)}
  <span class="slider-chip">
    <svg viewBox="0 0 24 24" aria-hidden="true"><ControlGlyph {id} /></svg>
    <span class="track"><i></i></span>
    <span class="chip-label">{controlLabel(id)}</span>
  </span>
{/snippet}

<div class="shell">
<div class="app">
  <nav class="nav" aria-label="Settings sections">
    <div class="brand">
      <span class="brand-mark" aria-hidden="true"></span>
      <span>Notchy</span>
    </div>
    <ul>
      {#each SECTIONS as entry (entry.id)}
        <li>
          <button
            type="button"
            class="nav-item"
            aria-current={section === entry.id ? "page" : undefined}
            onclick={() => open(entry.id)}
          >
            <span class="tile" style="--c: {entry.color}"><Glyph name={entry.id} size={14} /></span>
            <span class="nav-label">{entry.label}</span>
          </button>
        </li>
      {/each}
    </ul>
  </nav>

  <main class="content" bind:this={content}>
    {#if loaded}
      {#key section}
        <div class="section" in:rise={sectionIn}>
          <header class="head">
            <span class="tile big" style="--c: {current.color}"><Glyph name={current.id} size={22} /></span>
            <div>
              <h1>{current.label}</h1>
              <p>{current.blurb}</p>
            </div>
          </header>

          {#if section === "features"}
            <div class="feature-grid">
              {@render feature("Lyrics", "Synced lines while music plays.", "#FF375F", lyrics, (on) => { lyrics = on; scheduleSave(); }, artLyrics)}
              {@render feature("Volume and brightness", "A slim bar when you change either.", "#7DD3FC", showHud, (on) => { showHud = on; scheduleSave(); }, artHud)}
              {@render feature("Clock when idle", "The time in the resting notch.", "#FFFFFF", idleClock, (on) => { idleClock = on; scheduleSave(); }, artClock)}
              {@render feature("Notifications", "Other apps' alerts, in the notch.", "#0A84FF", notifyOn, (on) => { notifyOn = on; scheduleSave(); }, artNotice, "notifications")}
              {@render feature("Battery alerts", "Charger, full and low battery.", "#30D158", batteryOn, (on) => { batteryOn = on; scheduleSave(); }, artBattery, "battery", $batteryStatus === null)}
              {@render feature("System stats", "CPU, memory, network and disk.", "#0A84FF", statsOn, (on) => { statsOn = on; scheduleSave(); }, artStats)}
              {@render feature("Calendar reminders", "A heads-up before each event.", "#FF6A3D", calendarOn, (on) => { calendarOn = on; scheduleSave(); }, artCalendar, "calendar")}
              {@render feature("Timer sound", "A chime when a timer ends.", "#FF9F0A", timerSound, (on) => { timerSound = on; scheduleSave(); }, artTimer, "timer")}
              {@render feature("Shelf", "Copied text and dropped files.", "#6CB8FF", shelfClipboard || shelfFiles, (on) => { shelfFiles = on; shelfClipboard = on && shelfClipboard; scheduleSave(); }, artShelf, "shelf")}
              {@render feature("Global hotkeys", "Shortcuts that work in every app.", "#8E8E93", hotkeys.enabled, (on) => { hotkeys = { ...hotkeys, enabled: on }; scheduleSave(); }, artKeys, "hotkeys")}
            </div>
          {:else if section === "layout"}
            <p id="strip-help" class="help">
              Drag to change the order, or select one and use the arrow keys. Hover and press the minus to remove it.
            </p>

            <section class="card editor" aria-labelledby="pages-title">
              <div class="card-head">
                <h2 id="pages-title">Pages</h2>
                <span class="count">{shownPages.length} of {pageIds.length}</span>
              </div>
              <p class="card-sub">Swipe or scroll on the open notch to move through these, left to right.</p>
              <div class="stage">
                <div class="notch">
                  <Strip
                    ids={shownPages}
                    label="Pages in the notch"
                    labelOf={(id) => PAGE_LABELS[id as Page]}
                    chip={pageChip}
                    canRemove={() => shownPages.length > 1}
                    onreorder={arrangePages}
                    onremove={(id) => showPage(id, false)}
                    announce={(text) => (announcement = text)}
                  />
                  <div class="dots" aria-hidden="true">
                    {#each shownPages as id (id)}<i></i>{/each}
                  </div>
                </div>
              </div>
              {#if hiddenPages.length > 0}
                <div class="tray" role="group" aria-label="Pages you can add">
                  <span class="tray-label">Add</span>
                  {#each hiddenPages as row (row.id)}
                    <button type="button" class="add" onclick={() => showPage(row.id, true)}>
                      <Glyph name="plus" size={12} />
                      {@render pageChip(row.id)}
                    </button>
                  {/each}
                </div>
              {/if}
            </section>

            <section class="card editor" aria-labelledby="controls-title">
              <div class="card-head">
                <h2 id="controls-title">Controls page</h2>
                <span class="count">{shownTiles.length} of {MAX_TILES} buttons</span>
              </div>
              <p class="card-sub">Sliders sit on top, buttons and toggles below, as in the notch.</p>
              <div class="stage">
                <div class="notch controls-notch">
                  {#if shownSliders.length > 0}
                    <Strip
                      ids={shownSliders}
                      label="Sliders in the notch"
                      labelOf={controlLabel}
                      chip={sliderChip}
                      onreorder={(ids) => arrangeControls(ids, shownTiles)}
                      onremove={(id) => showControl(id, false)}
                      announce={(text) => (announcement = text)}
                    />
                  {/if}
                  {#if shownTiles.length > 0}
                    <Strip
                      ids={shownTiles}
                      label="Buttons in the notch"
                      labelOf={controlLabel}
                      chip={tileChip}
                      onreorder={(ids) => arrangeControls(shownSliders, ids)}
                      onremove={(id) => showControl(id, false)}
                      announce={(text) => (announcement = text)}
                    />
                  {:else}
                    <p class="stage-empty">No buttons. Add some below.</p>
                  {/if}
                </div>
              </div>
              {#if hiddenControls.length > 0}
                <div class="tray" role="group" aria-label="Controls you can add">
                  <span class="tray-label">Add</span>
                  {#each hiddenControls as row (row.id)}
                    {@const block = controlBlock(row.id, shownSliders.length, shownTiles.length)}
                    <button type="button" class="add" disabled={block !== null} title={block ?? undefined} onclick={() => showControl(row.id, true)}>
                      <Glyph name="plus" size={12} />
                      <span class="add-label">{controlLabel(row.id)}</span>
                      {#if block}<span class="add-why">{block}</span>{/if}
                    </button>
                  {/each}
                </div>
              {/if}
            </section>
            <p class="sr-only" role="status" aria-live="polite">{announcement}</p>
          {:else if section === "appearance"}
            <div class="font-preview" aria-hidden="true">
              <div class="mock-notch">
                <span class="m-art big"></span>
                <span class="preview-text">
                  <b>Blinding Lights</b>
                  <span>The Weeknd</span>
                </span>
              </div>
            </div>
            <div class="font-grid" role="radiogroup" aria-label="Font">
              {#each FONT_OPTIONS as option (option.id)}
                {@const chosen = font === option.id}
                <button
                  type="button"
                  role="radio"
                  class="font-card"
                  class:chosen
                  data-font={option.id}
                  aria-checked={chosen}
                  tabindex={chosen ? 0 : -1}
                  onclick={() => chooseFont(option.id)}
                  onkeydown={fontKeys}
                >
                  <span class="aa" style="font-family: {option.stack}">Aa</span>
                  <span class="font-name">{option.label}</span>
                </button>
              {/each}
            </div>
          {:else if section === "timer"}
            <div class="card">
              <div class="row">
                {@render rowHead("sound", "#FF9F0A", "Sound when a timer ends")}
                <Switch checked={timerSound} label="Sound when a timer ends" onchange={(on) => { timerSound = on; scheduleSave(); }} />
              </div>
            </div>
            <h2 class="group-title">Pomodoro</h2>
            <div class="length-grid">
              {#each LENGTHS as entry (entry.id)}
                {@const value = lengthOf(entry.id)}
                <div class="length" style="--c: {entry.color}">
                  <span class="length-name"><i></i>{entry.label}</span>
                  <div class="stepper">
                    <button type="button" aria-label="{entry.label} one minute shorter" disabled={value <= 1} onclick={() => setLength(entry.id, value - 1)}>
                      <Glyph name="minus" size={14} />
                    </button>
                    <label class="minutes">
                      <input
                        type="number"
                        inputmode="numeric"
                        min="1"
                        max={entry.max}
                        {value}
                        aria-label="{entry.label} length in minutes"
                        onchange={(event) => {
                          setLength(entry.id, readMinutes(event.currentTarget, value, 1, entry.max));
                          event.currentTarget.value = String(lengthOf(entry.id));
                        }}
                      />
                      <span>min</span>
                    </label>
                    <button type="button" aria-label="{entry.label} one minute longer" disabled={value >= entry.max} onclick={() => setLength(entry.id, value + 1)}>
                      <Glyph name="plus" size={14} />
                    </button>
                  </div>
                </div>
              {/each}
            </div>
            <p class="note">A long break comes after four focus sessions.</p>
          {:else if section === "calendar"}
            <div class="card">
              <div class="row">
                {@render rowHead("calendar", "#FF6A3D", "Reminders for events", "Kept on this PC, never sent anywhere")}
                <Switch checked={calendarOn} label="Reminders for events" onchange={(on) => { calendarOn = on; scheduleSave(); }} />
              </div>
              <div class="row" class:disabled={!calendarOn}>
                {@render rowHead("clock", "#FF9F0A", "Remind me")}
                <select
                  class="choice"
                  aria-label="Remind before an event"
                  disabled={!calendarOn}
                  value={String(calendarLead)}
                  onchange={(event) => {
                    calendarLead = Number(event.currentTarget.value);
                    scheduleSave();
                  }}
                >
                  {#each LEAD_OPTIONS as lead (lead)}
                    <option value={String(lead)}>{leadLabel(lead)}</option>
                  {/each}
                </select>
              </div>
              <div class="row" class:disabled={!calendarOn}>
                {@render rowHead("features", "#FFD60A", "All-day events at")}
                <select
                  class="choice"
                  aria-label="All-day events remind at this hour"
                  disabled={!calendarOn}
                  value={String(calendarHour)}
                  onchange={(event) => {
                    calendarHour = Number(event.currentTarget.value);
                    scheduleSave();
                  }}
                >
                  {#each HOURS as hour (hour)}
                    <option value={String(hour)}>{hourLabel(hour)}</option>
                  {/each}
                </select>
              </div>
            </div>
            <h2 class="group-title">Notes</h2>
            <CalendarMonth {addRequest} />
          {:else if section === "notifications"}
            <div class="card">
              <div class="row">
                {@render rowHead("notifications", "#FF453A", "Show notifications from other apps", notifyOn ? accessCopy($notificationsStatus.access) : "Read on this PC and never saved")}
                <Switch checked={notifyOn} label="Show notifications from other apps" onchange={(on) => { notifyOn = on; scheduleSave(); }} />
              </div>
            </div>
            <h2 class="group-title">Apps</h2>
            {#if $notificationsStatus.apps.length === 0}
              <div class="card empty">
                <Glyph name="notifications" size={22} />
                <p>Apps show up here after they send a notification.</p>
              </div>
            {:else}
              <div class="app-grid" class:disabled={!notifyOn}>
                {#each $notificationsStatus.apps as app (app.id)}
                  <div class="app-row">
                    <span class="avatar" aria-hidden="true">{app.name.slice(0, 1).toUpperCase()}</span>
                    <span class="label">{app.name}</span>
                    <Switch checked={isAllowed(app.id, deniedApps)} label="Show notifications from {app.name}" disabled={!notifyOn} onchange={(on) => toggleApp(app.id, on)} />
                  </div>
                {/each}
              </div>
            {/if}
          {:else if section === "battery"}
            <div class="card">
              <div class="row">
                {@render rowHead("battery", "#30D158", "Battery alerts", statusCopy($batteryStatus))}
                <Switch checked={batteryOn} label="Show battery alerts" disabled={$batteryStatus === null} onchange={(on) => { batteryOn = on; scheduleSave(); }} />
              </div>
              <div class="row" class:disabled={!batteryOn || !$batteryStatus}>
                {@render rowHead("bolt", "#30D158", "Charger and full charge")}
                <Switch checked={batteryCharge} label="Charger and full charge" disabled={!batteryOn || !$batteryStatus} onchange={(on) => { batteryCharge = on; scheduleSave(); }} />
              </div>
              <div class="row" class:disabled={!batteryOn || !$batteryStatus}>
                {@render rowHead("low", "#FF9F0A", "Low battery warnings", "At 20% and 10%")}
                <Switch checked={batteryLow} label="Low battery warnings" disabled={!batteryOn || !$batteryStatus} onchange={(on) => { batteryLow = on; scheduleSave(); }} />
              </div>
            </div>
          {:else if section === "shelf"}
            <div class="card">
              <div class="row">
                {@render rowHead("clipboard", "#6CB8FF", "Remember copied text", "Memory only, cleared on quit. Passwords are skipped")}
                <Switch checked={shelfClipboard} label="Remember copied text" onchange={(on) => { shelfClipboard = on; scheduleSave(); }} />
              </div>
              <div class="row">
                {@render rowHead("tray", "#5E5CE6", "Accept dropped files", "Drag a file onto the notch to keep it here")}
                <Switch checked={shelfFiles} label="Accept dropped files" onchange={(on) => { shelfFiles = on; scheduleSave(); }} />
              </div>
            </div>
          {:else if section === "hotkeys"}
            <div class="card">
              <div class="row">
                {@render rowHead("hotkeys", "#8E8E93", "Global hotkeys", "Off until you turn them on")}
                <Switch checked={hotkeys.enabled} label="Global hotkeys" onchange={(on) => { hotkeys = { ...hotkeys, enabled: on }; scheduleSave(); }} />
              </div>
            </div>
            <h2 class="group-title">Shortcuts</h2>
            <div class="card" class:disabled={!hotkeys.enabled}>
              {#each HOTKEY_ACTIONS as action (action.id)}
                {@const error = hotkeys.enabled ? bindingError($hotkeysStatus, action.id) : null}
                <div class="row tall">
                  <span class="text">
                    <span class="label">{action.label}</span>
                    {#if error}<span class="sub error" role="status">{error}</span>{/if}
                  </span>
                  <input
                    class="accelerator"
                    type="text"
                    spellcheck="false"
                    autocomplete="off"
                    placeholder="Not set"
                    aria-label="{action.label} shortcut"
                    disabled={!hotkeys.enabled}
                    value={hotkeys[action.id]}
                    onchange={(event) => {
                      setHotkey(action.id, event.currentTarget.value);
                      event.currentTarget.value = hotkeys[action.id];
                    }}
                  />
                </div>
              {/each}
            </div>
            <p class="note">Type a combination such as Ctrl+Alt+Space. Clear a box to turn it off. A key another app already uses cannot be taken.</p>
          {:else if section === "general"}
            <div class="card">
              <div class="row">
                {@render rowHead("power", "#30D158", "Launch at login")}
                <Switch checked={launchAtLogin} label="Launch at login" onchange={(on) => { launchAtLogin = on; scheduleSave(); }} />
              </div>
              <div class="row">
                {@render rowHead("lock", "#FF9F0A", "Confirm before locking", "The Lock button asks first")}
                <Switch checked={confirmLock} label="Confirm before locking" onchange={(on) => { confirmLock = on; scheduleSave(); }} />
              </div>
            </div>
            <h2 class="group-title">Reset</h2>
            <div class="card">
              <div class="row">
                {@render rowHead("reset", "#FF453A", "Reset to defaults", "Every setting goes back to how it started")}
                {#if confirmingReset}
                  <span class="confirm" in:fade={{ duration: 150 }}>
                    <button type="button" class="plain" onclick={() => (confirmingReset = false)}>Cancel</button>
                    <button type="button" class="destructive" onclick={resetDefaults}>Reset</button>
                  </span>
                {:else}
                  <button type="button" class="plain" onclick={() => (confirmingReset = true)}>Reset…</button>
                {/if}
              </div>
            </div>
            <div class="about">
              <span class="brand-mark big" aria-hidden="true"></span>
              <p><b>Notchy</b> {version}</p>
              <p>Lyrics come from lrclib.net.</p>
            </div>
          {/if}
        </div>
      {/key}
    {/if}

    {#if status}
      <p class="status" role="alert">{status}</p>
    {/if}
  </main>
</div>
</div>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    background: #0b0b0c;
    color: #fff;
    font-family: var(--font);
    -webkit-font-smoothing: antialiased;
    overflow: hidden;
    user-select: none;
    color-scheme: dark;
  }

  :global(button:focus-visible),
  :global(input:focus-visible),
  :global(select:focus-visible) {
    outline: 2px solid #0a84ff;
    outline-offset: 2px;
  }

  /* The window is a sidebar and a scrolling pane. Below 720 px the sidebar becomes a row of tabs
     along the top, so the content keeps the full width. */
  /* The size container. A container query cannot restyle the element it measures, so the grid
     sits one level in. */
  .shell {
    height: 100vh;
    container-type: inline-size;
  }

  .app {
    --card: #161618;
    --card-line: rgb(255 255 255 / 8%);
    --text-2: rgb(255 255 255 / 60%);
    --text-3: rgb(255 255 255 / 40%);
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    height: 100%;
  }

  .nav {
    display: flex;
    flex-direction: column;
    min-height: 0;
    padding: 18px 10px;
    box-sizing: border-box;
    background: #111113;
    box-shadow: inset -1px 0 0 var(--card-line);
    overflow-y: auto;
    scrollbar-width: none;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 10px 16px;
    font-size: 17px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  /* The app mark: a little notch hanging from a top edge. */
  .brand-mark {
    width: 22px;
    height: 16px;
    border-radius: 5px;
    background:
      linear-gradient(#000, #000) 50% 0 / 54% 42% no-repeat,
      linear-gradient(160deg, #5e5ce6, #0a84ff);
  }

  .brand-mark.big {
    width: 44px;
    height: 32px;
    border-radius: 9px;
  }

  .nav ul {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 34px;
    padding: 0 8px;
    border: 0;
    border-radius: 8px;
    background: none;
    color: rgb(255 255 255 / 85%);
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
    white-space: nowrap;
  }

  .nav-item:hover {
    background: rgb(255 255 255 / 6%);
  }

  .nav-item[aria-current="page"] {
    background: rgb(255 255 255 / 12%);
    color: #fff;
    font-weight: 600;
  }

  /* The coloured icon tile, used in the sidebar, the section heading and the settings rows. */
  .tile {
    display: grid;
    flex: none;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 7px;
    background: linear-gradient(180deg, color-mix(in oklab, var(--c), #fff 12%), var(--c));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 25%);
    color: #fff;
  }

  .row .tile {
    width: 30px;
    height: 30px;
    border-radius: 8px;
  }

  .tile.big {
    width: 48px;
    height: 48px;
    border-radius: 13px;
  }

  .content {
    min-height: 0;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: rgb(255 255 255 / 20%) transparent;
  }

  .section {
    box-sizing: border-box;
    max-width: 880px;
    margin: 0 auto;
    padding: 28px 32px 40px;
  }

  .head {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 22px;
  }

  h1 {
    margin: 0;
    font-size: 24px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }

  .head p {
    margin: 2px 0 0;
    font-size: 13px;
    color: var(--text-2);
  }

  .group-title {
    margin: 22px 4px 8px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-2);
  }

  .card {
    border-radius: 14px;
    background: var(--card);
    box-shadow: inset 0 0 0 0.5px var(--card-line);
    overflow: hidden;
  }

  .card.disabled,
  .app-grid.disabled {
    opacity: 0.5;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    min-height: 60px;
    padding: 10px 16px;
  }

  .row + .row {
    box-shadow: inset 0 1px 0 var(--card-line);
  }

  .row.disabled .tile,
  .row.disabled .text {
    opacity: 0.45;
  }

  .text {
    display: flex;
    flex: 1;
    min-width: 0;
    flex-direction: column;
    gap: 2px;
  }

  .label {
    font-size: 14px;
    line-height: 19px;
  }

  .sub {
    font-size: 12px;
    line-height: 16px;
    color: var(--text-2);
  }

  .sub.error {
    color: #ff453a;
  }

  .note,
  .help {
    margin: 10px 4px 0;
    font-size: 12px;
    line-height: 17px;
    color: var(--text-2);
  }

  .help {
    margin: -10px 0 14px;
  }

  /* --- Features */
  .feature-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(230px, 1fr));
    gap: 12px;
  }

  .feature {
    position: relative;
    display: flex;
    flex-direction: column;
    border-radius: 16px;
    background: var(--card);
    box-shadow: inset 0 0 0 0.5px var(--card-line);
    overflow: hidden;
  }

  /* The picture sits on a soft glow of the feature's colour, as a notch would on a wallpaper. */
  .art {
    position: relative;
    display: grid;
    place-items: start center;
    height: 92px;
    background:
      radial-gradient(90% 120% at 50% 130%, color-mix(in srgb, var(--c) 45%, transparent), transparent 70%),
      #0e0e10;
    transition: opacity 200ms ease;
  }

  .feature.off .art {
    opacity: 0.45;
  }

  .feature-body {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px 14px;
  }

  .feature-text {
    flex: 1;
    min-width: 0;
  }

  .feature h3 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }

  .feature p {
    margin: 2px 0 0;
    font-size: 12px;
    line-height: 16px;
    color: var(--text-2);
  }

  .more {
    position: absolute;
    top: 8px;
    right: 8px;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 6px 0 9px;
    border: 0;
    border-radius: 999px;
    background: rgb(0 0 0 / 55%);
    color: rgb(255 255 255 / 85%);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
    backdrop-filter: blur(8px);
  }

  .more:hover {
    background: rgb(0 0 0 / 80%);
    color: #fff;
  }

  /* --- The little notch pictures */
  .mock {
    display: flex;
    align-items: center;
    gap: 6px;
    box-sizing: border-box;
    background: #000;
    color: #fff;
  }

  .mock.pill {
    height: 24px;
    padding: 0 10px;
    border-radius: 0 0 12px 12px;
  }

  .mock.pill.wide {
    min-width: 128px;
  }

  .mock.pill.small {
    min-width: 72px;
    justify-content: center;
  }

  .mock.panel {
    width: 150px;
    height: 58px;
    padding: 8px 12px;
    border-radius: 0 0 16px 16px;
  }

  .mock.panel.shelf {
    justify-content: center;
    gap: 6px;
  }

  .mock.panel.shelf i {
    width: 36px;
    height: 36px;
    border-radius: 7px;
    background:
      radial-gradient(120% 80% at 0 0, color-mix(in srgb, var(--k) 45%, transparent), transparent 70%),
      rgb(255 255 255 / 10%);
  }

  .m-art {
    flex: none;
    width: 38px;
    height: 38px;
    border-radius: 8px;
    background: linear-gradient(135deg, #ff5f6d, #5b2bff);
    box-shadow: 0 2px 10px rgb(255 95 109 / 40%);
  }

  .m-art.big {
    width: 46px;
    height: 46px;
    border-radius: 50%;
  }

  .m-lines {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .m-lines.row {
    flex-direction: row;
  }

  .m-lines i {
    display: block;
    height: 4px;
    border-radius: 2px;
    background: #fff;
  }

  .m-lines i.dim {
    opacity: 0.4;
  }

  .m-bar {
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: rgb(255 255 255 / 16%);
    overflow: hidden;
  }

  .m-bar i {
    display: block;
    height: 100%;
  }

  .m-text {
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .m-text.green {
    margin-left: auto;
    color: #30d158;
  }

  .m-dot {
    display: grid;
    place-items: center;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    font-size: 8px;
    font-weight: 700;
  }

  .m-batt {
    position: relative;
    width: 20px;
    height: 10px;
    box-sizing: border-box;
    border: 1.2px solid rgb(255 255 255 / 50%);
    border-radius: 3px;
    padding: 1px;
  }

  .m-batt i {
    display: block;
    height: 100%;
    border-radius: 1px;
    background: #30d158;
  }

  .m-ring {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: conic-gradient(#ffd60a, #ff9f0a 70%, rgb(255 255 255 / 16%) 0);
    -webkit-mask: radial-gradient(circle, transparent 3.5px, #000 4px);
    mask: radial-gradient(circle, transparent 3.5px, #000 4px);
  }

  .m-spark {
    width: 100%;
    height: 100%;
    fill: none;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .keys {
    display: flex;
    gap: 6px;
    margin-top: 30px;
  }

  kbd {
    padding: 4px 8px;
    border-radius: 6px;
    background: linear-gradient(#2c2c2e, #1c1c1e);
    box-shadow:
      inset 0 1px 0 rgb(255 255 255 / 12%),
      0 2px 0 #000;
    font: 600 11px/1 var(--font);
    color: rgb(255 255 255 / 85%);
  }

  /* --- Notch layout editor */
  .editor {
    padding: 16px;
  }

  .editor + .editor {
    margin-top: 14px;
  }

  .card-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 12px;
  }

  .card-head h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .count {
    flex: none;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgb(255 255 255 / 8%);
    font-size: 11px;
    font-weight: 600;
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }

  .card-sub {
    margin: 4px 0 14px;
    font-size: 12px;
    color: var(--text-2);
  }

  /* The editor's stage: the notch hanging from the top of a soft "screen". */
  .stage {
    display: flex;
    justify-content: center;
    padding: 0 12px 18px;
    border-radius: 12px;
    background:
      radial-gradient(80% 120% at 50% 140%, rgb(94 92 230 / 30%), transparent 70%),
      #0e0e10;
  }

  .notch {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    box-sizing: border-box;
    width: min(100%, 560px);
    padding: 22px 18px 12px;
    border-radius: 0 0 26px 26px;
    background: #000;
    box-shadow: 0 10px 30px rgb(0 0 0 / 45%);
  }

  .dots {
    display: flex;
    gap: 6px;
  }

  .dots i {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: rgb(255 255 255 / 35%);
  }

  .dots i:first-child {
    background: #fff;
  }

  .stage-empty {
    margin: 8px 0;
    font-size: 12px;
    color: var(--text-3);
  }

  .page-chip {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    width: 80px;
    height: 70px;
    border-radius: 12px;
    background:
      radial-gradient(120% 80% at 50% 0, color-mix(in srgb, var(--c) 22%, transparent), transparent 70%),
      rgb(255 255 255 / 8%);
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 10%);
  }

  .page-chip.muted {
    opacity: 0.55;
  }

  .chip-icon {
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 9px;
    background: linear-gradient(180deg, color-mix(in oklab, var(--c), #fff 12%), var(--c));
    box-shadow: inset 0 1px 0 rgb(255 255 255 / 25%);
    color: #fff;
  }

  .chip-label {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    font-weight: 600;
    color: rgb(255 255 255 / 90%);
  }

  /* The page's place in the swipe order. */
  .chip-order {
    position: absolute;
    top: 5px;
    left: 6px;
    font-size: 10px;
    font-weight: 700;
    line-height: 12px;
    color: rgb(255 255 255 / 45%);
    font-variant-numeric: tabular-nums;
  }

  .chip-note {
    position: absolute;
    top: 4px;
    right: 4px;
    padding: 0 4px;
    border-radius: 4px;
    background: rgb(255 255 255 / 14%);
    font-size: 9px;
    font-weight: 700;
    line-height: 14px;
  }

  .tile-chip {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 5px;
    width: 58px;
  }

  .round {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    background: rgb(255 255 255 / 12%);
    color: #fff;
  }

  .round svg,
  .slider-chip svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .tile-chip.muted {
    opacity: 0.4;
  }

  .tile-chip .chip-label {
    max-width: 58px;
    font-size: 10px;
    font-weight: 500;
  }

  .slider-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    box-sizing: border-box;
    width: 220px;
    height: 34px;
    padding: 0 12px;
    border-radius: 12px;
    background: rgb(255 255 255 / 8%);
  }

  .slider-chip .track {
    flex: 1;
    height: 6px;
    border-radius: 3px;
    background: rgb(255 255 255 / 16%);
    overflow: hidden;
  }

  .slider-chip .track i {
    display: block;
    width: 60%;
    height: 100%;
    background: #fff;
  }

  .slider-chip .chip-label {
    width: 70px;
    text-align: right;
    font-weight: 500;
  }

  .tray {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }

  .tray-label {
    margin-right: 2px;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-3);
  }

  .add {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 4px 6px;
    border: 1px dashed rgb(255 255 255 / 18%);
    border-radius: 12px;
    background: none;
    color: rgb(255 255 255 / 85%);
    font: inherit;
    cursor: pointer;
    transition:
      transform var(--spring-snappy-ms) var(--spring-snappy),
      opacity 150ms ease;
  }

  .add:hover:not(:disabled) {
    border-style: solid;
    border-color: rgb(255 255 255 / 30%);
    transform: translateY(-1px);
  }

  .add:active:not(:disabled) {
    transform: scale(0.97);
  }

  .add:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* A page chip in the tray is drawn smaller, so the tray stays one row longer. */
  .add .page-chip {
    flex-direction: row;
    width: auto;
    height: 30px;
    padding: 0 8px 0 4px;
    background: none;
    box-shadow: none;
  }

  .add .page-chip .chip-icon {
    width: 22px;
    height: 22px;
    border-radius: 6px;
  }

  .add-label {
    font-size: 12px;
    font-weight: 600;
    line-height: 22px;
  }

  .add-why {
    font-size: 11px;
    color: var(--text-3);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }

  /* --- Appearance */
  .font-preview {
    display: flex;
    justify-content: center;
    padding: 0 0 26px;
    border-radius: 14px;
    background:
      radial-gradient(80% 120% at 50% 140%, rgb(191 90 242 / 35%), transparent 70%),
      #0e0e10;
  }

  .mock-notch {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 26px 16px 16px;
    border-radius: 0 0 24px 24px;
    background: #000;
  }

  .preview-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 15px;
  }

  .preview-text b {
    font-size: 19px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .preview-text span {
    color: var(--text-2);
  }

  .font-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 10px;
    margin-top: 16px;
  }

  .font-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 14px;
    border: 0;
    border-radius: 14px;
    background: var(--card);
    box-shadow: inset 0 0 0 0.5px var(--card-line);
    color: #fff;
    font: inherit;
    cursor: pointer;
    transition: transform var(--spring-snappy-ms) var(--spring-snappy);
  }

  .font-card:hover {
    transform: translateY(-2px);
  }

  .font-card.chosen {
    box-shadow:
      inset 0 0 0 2px #0a84ff,
      0 6px 20px rgb(10 132 255 / 25%);
  }

  .aa {
    font-size: 32px;
    font-weight: 600;
    line-height: 1;
    letter-spacing: -0.02em;
  }

  .font-name {
    font-size: 12px;
    color: var(--text-2);
  }

  .font-card.chosen .font-name {
    color: #64b5ff;
    font-weight: 600;
  }

  /* --- Timer */
  .length-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 10px;
  }

  .length {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
    border-radius: 14px;
    background:
      radial-gradient(120% 90% at 0 0, color-mix(in srgb, var(--c) 18%, transparent), transparent 70%),
      var(--card);
    box-shadow: inset 0 0 0 0.5px var(--card-line);
  }

  .length-name {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
  }

  .length-name i {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--c);
    box-shadow: 0 0 8px var(--c);
  }

  .stepper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .stepper button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: rgb(255 255 255 / 10%);
    color: #fff;
    cursor: pointer;
    transition: transform var(--spring-snappy-ms) var(--spring-snappy);
  }

  .stepper button:active:not(:disabled) {
    transform: scale(0.9);
  }

  .stepper button:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .minutes {
    display: flex;
    align-items: baseline;
    gap: 4px;
    color: var(--text-2);
    font-size: 12px;
  }

  .minutes input {
    width: 52px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: #fff;
    font: inherit;
    font-size: 28px;
    font-weight: 600;
    text-align: right;
    font-variant-numeric: tabular-nums;
    -moz-appearance: textfield;
    appearance: textfield;
  }

  .minutes input::-webkit-inner-spin-button,
  .minutes input::-webkit-outer-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  /* --- Controls */
  .choice {
    flex: none;
    height: 32px;
    box-sizing: border-box;
    padding: 0 8px;
    border: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 10%);
    color: #fff;
    font: inherit;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    color-scheme: dark;
  }

  .choice:disabled {
    opacity: 0.4;
  }

  .row.tall {
    min-height: 60px;
  }

  .accelerator {
    flex: none;
    width: min(180px, 42%);
    height: 32px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 10%);
    box-shadow: inset 0 -1px 0 rgb(0 0 0 / 50%);
    color: #fff;
    font: 600 13px/1 var(--font);
    text-align: center;
    color-scheme: dark;
  }

  .accelerator::placeholder {
    color: var(--text-3);
    font-weight: 400;
  }

  .accelerator:disabled {
    opacity: 0.4;
  }

  .app-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
    gap: 8px;
  }

  .app-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    border-radius: 12px;
    background: var(--card);
    box-shadow: inset 0 0 0 0.5px var(--card-line);
  }

  .app-row .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .avatar {
    display: grid;
    flex: none;
    place-items: center;
    width: 30px;
    height: 30px;
    border-radius: 8px;
    background: linear-gradient(135deg, #0a84ff, #64d2ff);
    font-size: 13px;
    font-weight: 700;
  }

  .empty {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 18px 16px;
    color: var(--text-3);
  }

  .empty p {
    margin: 0;
    font-size: 13px;
    color: var(--text-2);
  }

  .confirm {
    display: flex;
    gap: 6px;
  }

  .plain,
  .destructive {
    flex: none;
    height: 30px;
    padding: 0 12px;
    border: 0;
    border-radius: 8px;
    background: rgb(255 255 255 / 10%);
    color: #fff;
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  .plain:hover {
    background: rgb(255 255 255 / 16%);
  }

  .destructive {
    background: #ff453a;
    font-weight: 600;
  }

  .about {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    margin-top: 32px;
    font-size: 12px;
    color: var(--text-3);
  }

  .about p {
    margin: 0;
  }

  .about b {
    color: #fff;
  }

  .about .brand-mark {
    margin-bottom: 6px;
  }

  .status {
    position: sticky;
    bottom: 12px;
    margin: 0 auto;
    width: fit-content;
    max-width: calc(100% - 32px);
    padding: 8px 14px;
    border-radius: 10px;
    background: #3a1714;
    box-shadow: inset 0 0 0 0.5px rgb(255 69 58 / 50%);
    font-size: 13px;
    color: #ff8a80;
  }

  /* Narrow windows: the sidebar turns into a scrolling row of tabs, and the padding tightens. */
  @container (max-width: 720px) {
    .app {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(0, 1fr);
    }

    .nav {
      flex-direction: row;
      align-items: center;
      gap: 8px;
      padding: 10px 12px;
      overflow-x: auto;
      overflow-y: hidden;
      box-shadow: inset 0 -1px 0 var(--card-line);
    }

    .brand {
      padding: 0 6px 0 2px;
    }

    .brand span:last-child {
      display: none;
    }

    .nav ul {
      flex-direction: row;
      gap: 4px;
    }

    .nav-item {
      height: 32px;
      padding: 0 10px 0 5px;
      border-radius: 999px;
    }

    .section {
      padding: 20px 16px 32px;
    }

    .head {
      margin-bottom: 16px;
    }

    h1 {
      font-size: 20px;
    }

    .tile.big {
      width: 40px;
      height: 40px;
      border-radius: 11px;
    }
  }

  /* Very narrow: rows stack their control under the text when there is no room beside it. */
  @container (max-width: 440px) {
    .feature-grid {
      grid-template-columns: minmax(0, 1fr);
    }

    .head p {
      display: none;
    }

    .accelerator {
      width: 120px;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .font-card,
    .font-card:hover,
    .add,
    .add:hover:not(:disabled),
    .stepper button {
      transition: none;
      transform: none;
    }
  }
</style>
