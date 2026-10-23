<script lang="ts">
  import { islandState, setLyricsSize, setWide, type IslandState } from "./stores/island";
  import { mediaState, next, playPause, previous } from "./stores/media";
  import { currentLine, lyricsState } from "./stores/lyrics";
  import {
    enabledPages,
    enterPanel,
    go,
    leavePanel,
    page,
    PAGE_LABELS,
    step,
    type Page,
  } from "./stores/pages";
  import { tint } from "./stores/tint";
  import { levels } from "./stores/audio";
  import { refresh as refreshControls } from "./stores/controls";
  import { hud } from "./stores/hud";
  import { notice } from "./stores/notifications";
  import { initialOf } from "./lib/notifications";
  import { battery } from "./stores/battery";
  import { batteryColor, batteryLabel, glyphFill, hasBolt, percentText } from "./lib/battery";
  import { reminder } from "./stores/calendar";
  import { relativeLabel } from "./lib/calendar";
  import { hudFill, hudIcon, hudLabel, type HudIcon } from "./lib/hud";
  import { activityLabel, alertCopy, cycleDots, palette, ringRotations } from "./lib/timer";
  import { lastFinish, timerState, timerView, addTime, dismissTimer } from "./stores/timer";
  import { config } from "./stores/config";
  import CalendarPage from "./CalendarPage.svelte";
  import MediaProgress from "./MediaProgress.svelte";
  import ControlsPage from "./ControlsPage.svelte";
  import IdleClock from "./IdleClock.svelte";
  import RollingClock from "./RollingClock.svelte";
  import TimerPage from "./TimerPage.svelte";
  import StatsPage from "./StatsPage.svelte";
  import { fade, fly } from "svelte/transition";
  import { islandPath } from "./lib/shape";
  import { DEFAULT_FIT, fitTrack } from "./lib/lyricsFit";
  import { createWheelGesture } from "./lib/wheel";

  type Shown = Exclude<IslandState, "hidden">;

  // Body sizes in logical px. Must match IslandState::pill_size in src-tauri/src/state.rs,
  // which drives the click-through hit area. The ears sit outside the body and are not hit-tested.
  // The tucked ear is 2.5 because its bottom radius is 2.5, and the ear must end above that curve.
  // The expanded and lyrics heights include the top safe zone and the pager row, and must stay under
  // the 240 px window. Each page's usable height is the height less 46 px: 14 px safe zone, 8 px
  // gap, 16 px pager, 8 px under the pager. See DESIGN.md, "Layout system".
  // The lyrics body is wider than the panel and is taller for its enlarged lines.
  const SIZES: Record<Shown, { w: number; h: number; ear: number }> = {
    idle: { w: 120, h: 30, ear: 10 },
    active: { w: 200, h: 30, ear: 10 },
    activity: { w: 220, h: 30, ear: 10 },
    hud: { w: 260, h: 30, ear: 10 },
    notification: { w: 260, h: 30, ear: 10 },
    battery: { w: 260, h: 30, ear: 10 },
    calendar: { w: 260, h: 30, ear: 10 },
    alert: { w: 320, h: 64, ear: 10 },
    expanded: { w: 380, h: 192, ear: 10 },
    lyrics: { w: 620, h: 224, ear: 10 },
    tucked: { w: 64, h: 5, ear: 2.5 },
  };

  // The lyrics layout is sized for the track: 620 x 224 at rest, wider and then taller when a line is
  // too long to show whole, and its text shrinks only as a last step. See lib/lyricsFit.ts. The
  // size is worked out for every line of the track, so it holds still while the lines change.
  let fontReady = 0;
  if (typeof document !== "undefined" && document.fonts) {
    void document.fonts.ready.then(() => (fontReady += 1));
  }
  let measureContext: CanvasRenderingContext2D | null = null;
  let measureFont = "";
  const measured = new Map<string, number>();

  function measureText(text: string, px: number, weight: number): number {
    measureContext ??= document.createElement("canvas").getContext("2d");
    if (!measureContext) return text.length * px * 0.55;
    // The app sets the face as the --font custom property on the root, not as font-family.
    const family = getComputedStyle(document.documentElement).getPropertyValue("--font").trim() || "system-ui, sans-serif";
    if (family !== measureFont) {
      measureFont = family;
      measured.clear();
    }
    const key = `${weight}|${px}|${text}`;
    let width = measured.get(key);
    if (width === undefined) {
      const face = `${weight} ${px}px ${family}`;
      measureContext.font = face;
      width = measureContext.measureText(text).width;
      // A bundled face that has not loaded yet measures in the fallback font. Such a width is not
      // kept: the face is loaded, and the fit runs again with the real widths.
      if (document.fonts.check(face)) {
        measured.set(key, width);
      } else {
        void document.fonts.load(face).then(
          () => (fontReady += 1),
          () => {},
        );
      }
    }
    return width;
  }

  // The font and the loaded faces are arguments only so that a change of either runs this again.
  function fitLyrics(lines: string[], _font: string, _ready: number) {
    return lines.length === 0 ? DEFAULT_FIT : fitTrack(lines, measureText);
  }

  $: lyricLines = $lyricsState.status === "synced" ? $lyricsState.lines.map((l) => l.text) : [];
  $: lyricsFit = fitLyrics(lyricLines, $config.font, fontReady);
  $: lyricsBox = { w: lyricsFit.w, h: lyricsFit.h };
  $: setLyricsSize(lyricsBox.w, lyricsBox.h);

  // The HUD row. Its icon, bar and value follow the last change the backend sent.
  $: hudIconName = hudIcon($hud);
  $: hudFraction = hudFill($hud);
  $: hudText = hudLabel($hud);
  $: hudMuted = $hud.kind === "volume" && $hud.muted;

  // The notification pill shows the newest notice from the queue, and its app's initial as the badge.
  $: noticeBadge = initialOf($notice.app);

  // The battery pill shows the newest event: its glyph fill and colour, and what happened. The key
  // changes with each event, so its text fades in place.
  $: batteryFill = glyphFill($battery.percent);
  $: batteryTint = batteryColor($battery.kind);
  $: batteryWords = batteryLabel($battery.kind);
  $: batteryLevel = percentText($battery.percent);
  $: batteryBolt = hasBolt($battery.kind);
  $: batteryKey = `${$battery.kind}|${$battery.percent}`;

  // Pages slide this far sideways as they change. Reduced motion keeps only the fade.
  const PAGE_SHIFT = 16;

  // The panel shows one synced lyric line, or the source app when there are no lyrics.
  // A line change rises 4px while it fades in. Reduced motion keeps the fade and drops the rise.
  const reduceMotion =
    typeof matchMedia === "function" &&
    matchMedia("(prefers-reduced-motion: reduce)").matches;
  const lineIn = { duration: 180, y: reduceMotion ? 0 : 4 };
  // The lyrics stack moves as one block: it rises 10px while it fades in, and the old block fades.
  const stackIn = { duration: 220, y: reduceMotion ? 0 : 10 };
  $: lyricLine =
    $lyricsState.status === "synced" ? ($currentLine?.current ?? "") : "";
  $: sourceLine =
    $lyricsState.status === "none" || $lyricsState.status === "plain"
      ? $mediaState.source
      : "";

  // The lyrics layout shows the three lines around the playback position. Before the first line
  // there is nothing around it, so the lines are blank and the layout holds its place.
  const NO_WINDOW = { prev: "", current: "", next: "" };
  $: lyricWindow = $currentLine ?? NO_WINDOW;
  $: stackKey = `${lyricWindow.prev}\u0000${lyricWindow.current}\u0000${lyricWindow.next}`;

  // The live activity is a running, paused or finished timer. It takes the compact pill from media.
  // Its ring and digits come from the timer store, which extrapolates between backend updates.
  $: timer = $timerView;
  // The activity pill's word and its pomodoro dots. Both come from the same view as the digits.
  $: timerWord = activityLabel(timer);
  $: timerDots = cycleDots(timer);
  // The alert card shows the newest countdown that ended, until its owner clears it.
  $: alertText = $lastFinish ? alertCopy($lastFinish.data, $timerState.snapshot) : null;
  $: alertColors = $lastFinish
    ? palette($lastFinish.data.mode, $lastFinish.data.phase)
    : palette("timer");
  // The ripple plays once each time a new card comes up. Zero means no card.
  $: alertKey = $islandState === "alert" && $lastFinish ? $lastFinish.id : 0;
  // The last ten seconds beat once per displayed second. The key changes with the digits.
  $: beatKey = timer.lastTen ? timer.label : "";

  // The compact pill shows the title, or the artist when the title is empty.
  $: activeLabel = $mediaState.title || $mediaState.artist;

  // The wide layout needs the media page with synced lines and a playing track. Hover is
  // decided in Rust, so this only says the layout is available, not that it is showing.
  $: setWide(
    $page === "media" &&
      $mediaState.active &&
      $lyricsState.status === "synced" &&
      $currentLine !== null,
  );

  // Hidden keeps the last visible size, so the next state springs up from where the island was.
  let shown: Shown = "idle";
  $: if ($islandState !== "hidden") shown = $islandState;

  // Each shape is mounted at its native size, so its corners are never scaled at rest.
  // The shown shape sits at scale 1. The others are parked at the shown size and hidden.
  // A state change then animates only transform and opacity, and the incoming shape
  // springs up from the previous state's size.
  //
  // Docking: every shape scales about the top centre of the window, so its top edge stays
  // on y=0. The shape is split into two nested elements so each axis gets its own easing.
  // Width (outer, scaleX) keeps a gentle spring. Height (inner .frame, scaleY) must not
  // overshoot. A parked tucked sliver starts at about 28x its height, and any overshoot
  // past 1 on that axis would drive the scale negative and flip the shape.
  // BLEED extends each outline this many px above the window top. See islandPath.
  const BLEED = 16;

  // The size of a shape. Only the lyrics layout changes, with the track (see lyricsBox).
  function sizeOf(shape: Shown): { w: number; h: number; ear: number } {
    return shape === "lyrics" ? { ...SIZES.lyrics, ...lyricsBox } : SIZES[shape];
  }

  // `_box` is unused. Passing lyricsBox makes the lyrics shape's style follow a new size.
  function shapeStyle(shape: Shown, from: Shown, _box?: unknown): string {
    const { w, h, ear } = sizeOf(shape);
    const sx = sizeOf(from).w / w;
    return `width:${w + 2 * ear}px;height:${h}px;margin-left:${-(w / 2 + ear)}px;--w:${w}px;--ear:${ear}px;transform:scaleX(${sx})`;
  }

  function frameStyle(shape: Shown, from: Shown, _box?: unknown): string {
    const sy = sizeOf(from).h / sizeOf(shape).h;
    return `transform:scaleY(${sy})`;
  }

  // Opening the panel picks its page and reloads the control values. Closing it remembers the page.
  // The lyrics layout is part of the open panel, so moving between the two does not reset it.
  let panelOpen = false;
  $: syncPanel($islandState);

  function syncPanel(state: IslandState) {
    const open = state === "expanded" || state === "lyrics";
    if (open && !panelOpen) {
      enterPanel();
      void refreshControls();
    } else if (!open && panelOpen) {
      leavePanel();
    }
    panelOpen = open;
  }

  // Each page sits PAGE_SHIFT to the side of the shown one, on the side it has to come from.
  // A page that becomes shown moves from its old offset to 0, and the one it replaces moves away.
  function pageOffset(id: Page): number {
    const pages = $enabledPages;
    return Math.sign(pages.indexOf(id) - pages.indexOf($page)) * PAGE_SHIFT;
  }

  // The wheel turns pages through the classifier in lib/wheel.ts. Mouse notches turn at once,
  // and a trackpad fling turns one page however long its inertia runs.
  const SWIPE_MIN_PX = 40;
  const wheel = createWheelGesture();
  let swipeFrom: { x: number; y: number } | null = null;

  function onWheel(event: WheelEvent) {
    const turn = wheel.feed(event);
    if (turn !== 0) step(turn);
  }

  function swipeStart(event: PointerEvent) {
    // Sliders and the progress bar keep their own drags, so a press on one never starts a swipe.
    swipeFrom = (event.target as Element).closest("input, [role='slider']")
      ? null
      : { x: event.clientX, y: event.clientY };
  }

  // Plays a soft ring-out once each time `key` moves to a new card. Transform and opacity only.
  function ripple(node: HTMLElement, key: number) {
    let last = key;
    return {
      update(next: number) {
        const fresh = next !== 0 && next !== last;
        last = next;
        if (!fresh || reduceMotion) return;
        node.animate(
          [
            { transform: "scale(1)", opacity: 0.85 },
            { transform: "scale(2.6)", opacity: 0 },
          ],
          { duration: 900, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
        );
      },
    };
  }

  // A one-step bump of the digits, once per displayed second in the last ten.
  function beat(node: HTMLElement, key: string) {
    let last = key;
    return {
      update(next: string) {
        const fresh = next !== "" && next !== last;
        last = next;
        if (!fresh || reduceMotion) return;
        node.animate(
          [
            { transform: "scale(1)" },
            { transform: "scale(1.06)" },
            { transform: "scale(1)" },
          ],
          { duration: 300, easing: "ease-out" },
        );
      },
    };
  }

  function swipeEnd(event: PointerEvent) {
    if (!swipeFrom) return;
    const dx = event.clientX - swipeFrom.x;
    const dy = event.clientY - swipeFrom.y;
    swipeFrom = null;
    if (Math.abs(dx) > SWIPE_MIN_PX && Math.abs(dx) > Math.abs(dy)) {
      step(dx < 0 ? 1 : -1);
    }
  }

</script>

{#snippet albumArt(size: "sm" | "lg" | "xl")}
  <div class="art {size}">
    {#if $mediaState.art}
      <img src={$mediaState.art} alt="" />
    {:else}
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path d="M9 18V5l12-2v13" />
        <circle cx="6" cy="18" r="3" />
        <circle cx="18" cy="16" r="3" />
      </svg>
    {/if}
  </div>
{/snippet}

{#snippet transport(compact: boolean)}
  <div class="controls" class:compact>
    <button type="button" aria-label="Previous" onclick={previous}>
      <svg viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M6 6h2v12H6zM18 6v12l-8.5-6z" /></svg
      >
    </button>
    <button
      type="button"
      class="play"
      aria-label={$mediaState.playing ? "Pause" : "Play"}
      onclick={playPause}
    >
      <svg viewBox="0 0 24 24" aria-hidden="true">
        {#if $mediaState.playing}
          <path d="M7 5h4v14H7zM13 5h4v14h-4z" />
        {:else}
          <path d="M8 5v14l11-7z" />
        {/if}
      </svg>
    </button>
    <button type="button" aria-label="Next" onclick={next}>
      <svg viewBox="0 0 24 24" aria-hidden="true"
        ><path d="M16 6h2v12h-2zM6 6v12l8.5-6z" /></svg
      >
    </button>
  </div>
{/snippet}

{#snippet pager()}
  <div class="pager">
    {#each $enabledPages as id (id)}
      <button
        type="button"
        class="dot"
        class:on={$page === id}
        aria-label="{PAGE_LABELS[id]} page"
        aria-current={$page === id ? "true" : undefined}
        onclick={() => go(id)}
      >
        <span></span>
      </button>
    {/each}
  </div>
{/snippet}

{#snippet hudGlyph(icon: HudIcon)}
  {#if icon === "sun"}
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
  {:else}
    <path d="M4 9v6h4l5 4V5L8 9H4z" />
    {#if icon === "low" || icon === "high"}
      <path d="M16 9.5a3.5 3.5 0 0 1 0 5" />
    {/if}
    {#if icon === "high"}
      <path d="M19 6.5a8 8 0 0 1 0 11" />
    {/if}
    {#if icon === "muted"}
      <path class="slash" d="M17 9l5 6M22 9l-5 6" />
    {/if}
  {/if}
{/snippet}

{#snippet ring(fraction: number, from: string, to: string)}
  {@const turn = ringRotations(fraction)}
  <span
    class="ring"
    style="--right: {turn.right}deg; --left: {turn.left}deg; --from: {from}; --to: {to}"
    aria-hidden="true"
  >
    <span class="track"></span>
    <span class="half right"><span class="disc"></span></span>
    <span class="half left"><span class="disc"></span></span>
    <span class="core"></span>
  </span>
{/snippet}

{#snippet outline(name: Shown, _box?: unknown)}
  {@const { w, h, ear } = sizeOf(name)}
  <svg
    class="outline"
    viewBox="0 {-BLEED} {w + 2 * ear} {h + BLEED}"
    aria-hidden="true"
  >
    <path d={islandPath(w, h, ear, Math.min(h / 2, 24), BLEED)} />
  </svg>
{/snippet}

<div class="island" class:hidden={$islandState === "hidden"}>
  <div
    class="shape idle"
    class:on={$islandState === "idle"}
    style={shapeStyle("idle", shown)}
  >
    <div class="frame" style={frameStyle("idle", shown)}>
      {@render outline("idle")}
      {#if $config.idleClock}
        <div class="content idle-row">
          <IdleClock />
        </div>
      {/if}
    </div>
  </div>

  <div
    class="shape active"
    class:on={$islandState === "active"}
    style={shapeStyle("active", shown)}
  >
    <div class="frame" style={frameStyle("active", shown)}>
      {@render outline("active")}
      <div class="content compact">
        {@render albumArt("sm")}
        <div class="active-title">
          {#key activeLabel}
            <span in:fade={{ duration: 200 }} out:fade={{ duration: 200 }}>
              {activeLabel}
            </span>
          {/key}
        </div>
        <div class="bars" class:live={$levels !== null} aria-hidden="true">
          {#each [0, 1, 2, 3] as bar (bar)}
            <span style={$levels ? `transform: scaleY(${$levels[bar]})` : undefined}></span>
          {/each}
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape activity"
    class:on={$islandState === "activity"}
    style={shapeStyle("activity", shown)}
    aria-hidden="true"
  >
    <div class="frame" style={frameStyle("activity", shown)}>
      {@render outline("activity")}
      <div
        class="content activity-row"
        class:paused={timer.status === "paused"}
        style="--c: {timer.palette.from}; --c2: {timer.palette.to}"
      >
        <div class="lead">
          {#if timer.mode === "stopwatch"}
            <svg class="stopwatch-glyph" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="12" cy="13.5" r="7.5" />
              <path d="M12 13.5V10M9.5 3h5" />
            </svg>
          {:else}
            {@render ring(timer.fraction, timer.palette.from, timer.palette.to)}
          {/if}
        </div>
        <div class="mid">
          <span class="word">{timerWord}</span>
          {#if timerDots.total > 0}
            <span class="cycle" aria-label="{timerDots.done} of {timerDots.total} focus sessions done">
              {#each Array(timerDots.total) as _, n (n)}
                <span class="cdot" class:on={n < timerDots.done}></span>
              {/each}
            </span>
          {/if}
        </div>
        <div class="clock-slot" use:beat={beatKey}>
          <RollingClock text={timer.label} />
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape hud"
    class:on={$islandState === "hud"}
    style={shapeStyle("hud", shown)}
    aria-hidden="true"
  >
    <div class="frame" style={frameStyle("hud", shown)}>
      {@render outline("hud")}
      <div class="content hud-row" class:brightness={$hud.kind === "brightness"}>
        <div class="hud-icon" class:muted={hudMuted}>
          {#key hudIconName}
            <svg class="glyph" viewBox="0 0 24 24" in:fade={{ duration: 160 }} out:fade={{ duration: 110 }}>
              {@render hudGlyph(hudIconName)}
            </svg>
          {/key}
        </div>
        <div class="hud-track">
          <span class="hud-fill" style="transform: scaleX({hudFraction})"></span>
        </div>
        <span class="hud-value">{hudText}</span>
      </div>
    </div>
  </div>

  <div
    class="shape notification"
    class:on={$islandState === "notification"}
    style={shapeStyle("notification", shown)}
  >
    <div class="frame" style={frameStyle("notification", shown)}>
      {@render outline("notification")}
      <div class="content notice-row">
        <span class="notice-badge" aria-hidden="true">{noticeBadge}</span>
        <div class="notice-copy" role="status">
          {#key $notice.seq}
            <p class="notice-text" in:fade={{ duration: 200 }} out:fade={{ duration: 120 }}>
              <span class="notice-title">{$notice.title}</span>{#if $notice.body}{" "}<span class="notice-body">{$notice.body}</span>{/if}
            </p>
          {/key}
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape battery"
    class:on={$islandState === "battery"}
    style="{shapeStyle('battery', shown)}; --c: {batteryTint}"
  >
    <div class="frame" style={frameStyle("battery", shown)}>
      {@render outline("battery")}
      <div class="content battery-row">
        <svg class="battery-glyph" viewBox="0 0 25 14" aria-hidden="true">
          <rect class="glyph-track" x="3" y="4" width="16" height="6" rx="1.5" />
          <rect
            class="glyph-fill"
            x="3"
            y="4"
            width="16"
            height="6"
            rx="1.5"
            style="transform: scaleX({batteryFill})"
          />
          <rect class="glyph-body" x="0.75" y="1.75" width="20.5" height="10.5" rx="3" />
          <rect class="glyph-nub" x="22.5" y="4.5" width="2" height="5" rx="1" />
          {#if batteryBolt}
            <path class="glyph-bolt" d="M11.6 4.5 9 7.4h1.9l-.7 2.1L13 6.6h-1.9z" />
          {/if}
        </svg>
        <div class="battery-copy" role="status">
          {#key batteryKey}
            <p class="battery-text" in:fade={{ duration: 200 }} out:fade={{ duration: 120 }}>
              <span class="battery-label">{batteryWords}</span>
              <span class="battery-level">{batteryLevel}</span>
            </p>
          {/key}
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape calendar"
    class:on={$islandState === "calendar"}
    style={shapeStyle("calendar", shown)}
  >
    <div class="frame" style={frameStyle("calendar", shown)}>
      {@render outline("calendar")}
      <div class="content calendar-row">
        <svg class="calendar-glyph" viewBox="0 0 16 16" aria-hidden="true">
          <rect x="1.5" y="2.5" width="13" height="12" rx="2.5" />
          <path d="M1.5 6.5h13M5 1v3M11 1v3" />
        </svg>
        <div class="calendar-copy" role="status">
          {#key $reminder.seq}
            <p class="calendar-text" in:fade={{ duration: 200 }} out:fade={{ duration: 120 }}>
              <span class="calendar-title">{$reminder.title}</span>
              <span class="calendar-when">{relativeLabel($reminder.minutesUntil)}</span>
            </p>
          {/key}
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape alert"
    class:on={$islandState === "alert"}
    style="{shapeStyle('alert', shown)}; --c: {alertColors.from}; --c2: {alertColors.to}"
    role="group"
    aria-label="Timer finished"
  >
    <div class="frame" style={frameStyle("alert", shown)}>
      <span class="glow" aria-hidden="true"></span>
      {@render outline("alert")}
      <div class="content alert-row">
        <div class="alert-ring" use:ripple={alertKey}>
          {@render ring(1, alertColors.from, alertColors.to)}
          <span class="ripple" aria-hidden="true"></span>
        </div>
        <div class="alert-copy" role="status">
          <div class="alert-title">{alertText?.title ?? ""}</div>
          <div class="alert-sub">{alertText?.detail ?? ""}</div>
        </div>
        <div class="alert-actions">
          <button type="button" class="pill" onclick={() => addTime(60_000)}>+1 min</button>
          <button type="button" class="pill strong" onclick={() => dismissTimer()}>Dismiss</button>
        </div>
      </div>
    </div>
  </div>

  <div
    class="shape expanded"
    class:on={$islandState === "expanded"}
    style={shapeStyle("expanded", shown)}
    role="group"
    aria-label="Island panel"
    onwheel={onWheel}
    onpointerdown={swipeStart}
    onpointerup={swipeEnd}
    onpointerleave={() => (swipeFrom = null)}
  >
    <div class="frame" style={frameStyle("expanded", shown)}>
      {@render outline("expanded")}
      <div class="content panel">
        <div class="pages">
          {#each $enabledPages as id (id)}
            <div
              class="page"
              class:on={$page === id}
              style="--x: {pageOffset(id)}px"
              inert={$page !== id}
            >
              {#if id === "timer"}
                <TimerPage />
              {:else if id === "calendar"}
                <CalendarPage />
              {:else if id === "media"}
                {#if $mediaState.active}
                  <div class="now">
                    {@render albumArt("lg")}
                    <div class="meta">
                      <div class="title">{$mediaState.title}</div>
                      <div class="artist">{$mediaState.artist}</div>
                      <MediaProgress />
                    </div>
                  </div>
                  <div class="lyric">
                    {#if lyricLine}
                      {#key lyricLine}
                        <p class="line" in:fly={lineIn} out:fade={{ duration: 180 }}>
                          {lyricLine}
                        </p>
                      {/key}
                    {:else if sourceLine}
                      <p class="source">Playing from {sourceLine}</p>
                    {/if}
                  </div>
                  {@render transport(false)}
                {:else}
                  <p class="empty">Nothing playing</p>
                {/if}
              {:else if id === "stats"}
                <StatsPage />
              {:else}
                <ControlsPage />
              {/if}
            </div>
          {/each}
        </div>
        {#if $enabledPages.length > 1}
          {@render pager()}
        {/if}
      </div>
    </div>
  </div>

  <div
    class="shape lyrics"
    class:on={$islandState === "lyrics"}
    style={shapeStyle("lyrics", shown, lyricsBox)}
    role="group"
    aria-label="Lyrics panel"
    onwheel={onWheel}
    onpointerdown={swipeStart}
    onpointerup={swipeEnd}
    onpointerleave={() => (swipeFrom = null)}
  >
    <div class="frame" style={frameStyle("lyrics", shown, lyricsBox)}>
      {@render outline("lyrics", lyricsBox)}
      {#key $tint.id}
        {@const { w, h, ear } = sizeOf("lyrics")}
        <svg
          class="outline"
          viewBox="0 {-BLEED} {w + 2 * ear} {h + BLEED}"
          aria-hidden="true"
          style="--tint: {$tint.color}"
          in:fade={{ duration: 400 }}
          out:fade={{ duration: 400 }}
        >
          <defs>
            <!-- The tint fades to black by the bottom, so the edge still blends into the screen. -->
            <linearGradient
              id="tint-{$tint.id}"
              gradientUnits="userSpaceOnUse"
              x1="0"
              y1="0"
              x2="0"
              y2={h}
            >
              <stop offset="0" style="stop-color: var(--tint)" />
              <stop offset="0.4" style="stop-color: color-mix(in srgb, var(--tint) 55%, #000)" />
              <stop offset="0.85" style="stop-color: #000" />
            </linearGradient>
          </defs>
          <path
            d={islandPath(w, h, ear, Math.min(h / 2, 24), BLEED)}
            fill="url(#tint-{$tint.id})"
          />
        </svg>
      {/key}
      <div class="content wide" style="--ls: {lyricsFit.scale}">
        <div class="wide-body">
          <div class="side">
            <div class="now-wide">
              {@render albumArt("xl")}
              <div class="meta">
                <div class="heading">{$mediaState.title}</div>
                <div class="artist">{$mediaState.artist}</div>
              </div>
            </div>
            <MediaProgress />
            {@render transport(true)}
          </div>
          <div class="stack-wrap">
            {#key stackKey}
              <div class="stack" in:fly={stackIn} out:fade={{ duration: 180 }}>
                <p class="ctx">{lyricWindow.prev}</p>
                <p class="cur"><span>{lyricWindow.current}</span></p>
                <p class="ctx">{lyricWindow.next}</p>
              </div>
            {/key}
          </div>
        </div>
        {#if $enabledPages.length > 1}
          {@render pager()}
        {/if}
      </div>
    </div>
  </div>

  <div
    class="shape tucked"
    class:on={$islandState === "tucked"}
    style={shapeStyle("tucked", shown)}
  >
    <div class="frame" style={frameStyle("tucked", shown)}>
      {@render outline("tucked")}
    </div>
  </div>
</div>

<style>
  /* Vertical motion never overshoots. A slide past its rest point would lift the top edge off y=0. */
  .island {
    --bleed: 16px; /* Matches BLEED in the script. */
    position: absolute;
    inset: 0;
    /* Only the shapes that are on take clicks, so the rest of the window passes through. */
    pointer-events: none;
    transition:
      transform 450ms cubic-bezier(0.22, 1, 0.36, 1),
      opacity 180ms ease;
  }

  /* Hidden slides the whole island above the window. No shape is on, so nothing takes clicks. */
  .island.hidden {
    opacity: 0;
    transform: translateY(-110%);
  }

  /* Width scale keeps a gentle spring (peak about 1.03). Scale is about the top centre. */
  .shape {
    position: absolute;
    top: 0;
    left: 50%;
    box-sizing: border-box;
    transform-origin: top center;
    opacity: 0;
    will-change: transform, opacity;
    transition:
      transform 450ms cubic-bezier(0.34, 1.3, 0.64, 1),
      opacity 180ms ease;
  }

  /* Height scale, which must not overshoot. See the note on frameStyle. */
  .frame {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    transform-origin: top center;
    will-change: transform;
    transition: transform 450ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .shape.on {
    opacity: 1;
    pointer-events: auto;
  }

  /* The outline starts BLEED px above the window top. The window clips at y=0. */
  .outline {
    position: absolute;
    top: calc(-1 * var(--bleed));
    left: 0;
    width: 100%;
    height: calc(100% + var(--bleed));
    overflow: visible;
    fill: #000;
  }

  /* Content leaves before its shape fades, and enters after the shape has started. */
  .content {
    position: absolute;
    top: 0;
    left: var(--ear);
    width: var(--w);
    height: 100%;
    box-sizing: border-box;
    color: #fff;
    font: 500 13px/1.2 var(--font);
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .shape.on .content {
    opacity: 1;
    transition: opacity 200ms ease 120ms;
  }

  .compact {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 10px;
  }

  .art {
    flex: none;
    display: grid;
    place-items: center;
    overflow: hidden;
    border-radius: 50%;
    background: #1c1c1e;
    color: rgba(255, 255, 255, 0.7);
  }

  .art.sm {
    width: 20px;
    height: 20px;
  }

  .art.lg {
    width: 72px;
    height: 72px;
  }

  .art.xl {
    width: 88px;
    height: 88px;
  }

  .art img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }

  .art svg {
    width: 60%;
    height: 60%;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* The title fills the room between the art and the bars. It fades at the right edge instead of
     ending in an ellipsis. The grid cell is the container's width, so a long title never grows
     the pill, and a track change crossfades two spans in the same cell. */
  .active-title {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    flex: 1;
    min-width: 0;
    overflow: hidden;
    -webkit-mask-image: linear-gradient(to right, #000 calc(100% - 16px), transparent);
    mask-image: linear-gradient(to right, #000 calc(100% - 16px), transparent);
  }

  .active-title span {
    grid-area: 1 / 1;
    font-size: 12px;
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    color: rgba(255, 255, 255, 0.9);
  }

  .bars {
    display: flex;
    flex: none;
    align-items: flex-end;
    gap: 2px;
    height: 14px;
  }

  .bars span {
    width: 3px;
    height: 100%;
    border-radius: 1.5px;
    background: #fff;
    transform-origin: bottom;
  }

  /* Animates only while the active shape is on. Idle, tucked and hidden run no animation. */
  .shape.active.on .bars span {
    animation: bar 900ms ease-in-out infinite alternate;
  }

  /* Live levels replace the fallback. The inline transform sets each bar, and a short ease
     smooths the 30 Hz frames. Only the transform moves. */
  .shape.active.on .bars.live span {
    animation: none;
    transition: transform 70ms ease-out;
  }

  .bars span:nth-child(1) {
    animation-delay: -300ms;
  }

  .bars span:nth-child(2) {
    animation-delay: -600ms;
  }

  .bars span:nth-child(3) {
    animation-delay: -150ms;
  }

  .bars span:nth-child(4) {
    animation-delay: -450ms;
  }

  @keyframes bar {
    from {
      transform: scaleY(0.2);
    }
    to {
      transform: scaleY(1);
    }
  }

  /* The HUD row. The bar is a track with a fill that scales from its left edge, so a key repeat
     glides to each new level. Only transform moves. The track's radius clips the fill's ends. */
  /* The idle pill's clock: dim, centred, digits that do not shift. Off in Settings means no row. */
  .idle-row {
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .hud-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 14px;
  }

  .hud-icon {
    position: relative;
    flex: none;
    width: 18px;
    height: 18px;
    color: #fff;
  }

  /* The icon swaps by crossfading two glyphs that sit on top of each other. */
  .hud-icon .glyph {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .hud-icon .slash {
    stroke: #ff6b6b;
  }

  .hud-icon.muted {
    color: rgba(255, 255, 255, 0.45);
  }

  .hud-row.brightness .hud-icon {
    color: #ffd166;
  }

  .hud-track {
    position: relative;
    flex: 1;
    min-width: 0;
    height: 4px;
    border-radius: 2px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.16);
  }

  .hud-fill {
    position: absolute;
    inset: 0;
    transform-origin: left center;
    transition: transform 120ms ease-out;
    background: linear-gradient(to right, #7dd3fc, #ffffff);
  }

  .hud-row.brightness .hud-fill {
    background: linear-gradient(to right, #ffd166, #fff3c4);
  }

  .hud-value {
    flex: none;
    width: 34px;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    text-align: right;
    color: rgba(255, 255, 255, 0.9);
  }

  /* The notification pill. Three columns, so the text sits in the middle with the badge on the left
     and an equal 16 px empty column on the right, which keeps the text centred in the pill. The
     badge is the notification accent. The text is one line and fades out with an ellipsis. The
     pill takes no clicks: nothing is wired to it. */
  .shape.notification.on {
    pointer-events: none;
  }

  .notice-row {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) 16px;
    align-items: center;
    gap: 8px;
    padding: 0 12px;
  }

  .notice-badge {
    justify-self: start;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: linear-gradient(90deg, #0a84ff, #64d2ff);
    color: #000;
    font-size: 9px;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
  }

  /* Each notice is laid over the same grid cell, so an outgoing text fades over the new one
     without moving the pill. */
  .notice-copy {
    display: grid;
    min-width: 0;
    overflow: hidden;
  }

  .notice-text {
    grid-area: 1 / 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 12px;
    font-weight: 600;
    line-height: 16px;
    text-align: center;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.9);
  }

  .notice-body {
    font-weight: 400;
    color: rgba(255, 255, 255, 0.6);
  }

  /* The battery pill. The glyph and its words are one group, centred in the pill, so the group's
     centre is the pill's centre. The glyph fill scales from its left edge and takes the kind's
     colour. The pill takes no clicks, as the notification pill does. */
  .shape.battery.on {
    pointer-events: none;
  }

  .battery-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 0 12px;
  }

  .battery-glyph {
    flex: none;
    width: 25px;
    height: 14px;
    overflow: visible;
  }

  .battery-glyph .glyph-track {
    fill: rgba(255, 255, 255, 0.16);
  }

  .battery-glyph .glyph-fill {
    fill: var(--c);
    transform-box: fill-box;
    transform-origin: left center;
    transition: transform 320ms cubic-bezier(0.22, 1, 0.36, 1);
  }

  .battery-glyph .glyph-body {
    fill: none;
    stroke: rgba(255, 255, 255, 0.85);
    stroke-width: 1.5;
  }

  .battery-glyph .glyph-nub {
    fill: rgba(255, 255, 255, 0.85);
  }

  /* The bolt is white with a thin black keyline, so it reads over the fill and over the track. */
  .battery-glyph .glyph-bolt {
    fill: #fff;
    stroke: #000;
    stroke-width: 0.8;
    stroke-linejoin: round;
    paint-order: stroke;
  }

  /* The text lines sit in one grid cell, so an outgoing line fades over the incoming one. */
  .battery-copy {
    display: grid;
    min-width: 0;
  }

  .battery-text {
    grid-area: 1 / 1;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 12px;
    line-height: 16px;
    white-space: nowrap;
  }

  .battery-label {
    font-weight: 600;
    color: rgba(255, 255, 255, 0.9);
  }

  .battery-level {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.6);
  }

  /* The calendar reminder pill. A calendar glyph in the calendar accent, then the event's title and
     the time left, centred as one group, so the group's centre is the pill's centre. The title
     shortens with an ellipsis. The pill takes no clicks, as the notification pill does. */
  .shape.calendar.on {
    pointer-events: none;
  }

  .calendar-row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 0 12px;
  }

  .calendar-glyph {
    flex: none;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: #ff6a3d;
    stroke-width: 1.5;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .calendar-copy {
    display: grid;
    min-width: 0;
    overflow: hidden;
  }

  .calendar-text {
    grid-area: 1 / 1;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin: 0;
    font-size: 12px;
    line-height: 16px;
    white-space: nowrap;
  }

  .calendar-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.9);
  }

  .calendar-when {
    flex: none;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.6);
  }

  .panel {
    display: flex;
    flex-direction: column;
    padding: 14px 16px 0;
  }

  /* Pages overlap in one box. The shown page sits in place. Each other page waits PAGE_SHIFT
     to a side with opacity 0, so a page change slides one in and the other out. The 8 px bottom
     margin keeps every page's content clear of the pager row. */
  .pages {
    position: relative;
    flex: 1;
    min-height: 0;
    margin-bottom: 8px;
  }

  /* Each page's content is centred in the page box, so the space above and below it is equal.
     A page never pins its controls to the bottom edge. */
  .page {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 8px;
    transform: translateX(var(--x, 0px));
    opacity: 0;
    transition:
      transform 220ms cubic-bezier(0.22, 1, 0.36, 1),
      opacity 220ms ease;
  }

  .page.on {
    transform: none;
    opacity: 1;
  }

  .page:not(.on) {
    pointer-events: none;
  }

  /* Dots take 16px hit areas. The dot itself is 5px. The row sits 8 px above the bottom edge. */
  .pager {
    display: flex;
    flex: none;
    justify-content: center;
    gap: 4px;
    height: 16px;
    margin-bottom: 8px;
  }

  .dot {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
  }

  .dot span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #fff;
    opacity: 0.35;
    transition: opacity 220ms ease;
  }

  .dot.on span {
    opacity: 1;
  }

  .dot:focus-visible {
    outline: 1px solid rgba(255, 255, 255, 0.6);
  }

  .now {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  .meta {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .title,
  .artist {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .title {
    font-size: 14px;
    font-weight: 600;
  }

  .artist {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
  }

  /* A fixed row, so a line that is missing or changing never moves the controls. The line and
     the source share one cell, which lets an outgoing line fade while the next one rises. */
  .lyric {
    display: grid;
    height: 16px;
  }

  .lyric > * {
    grid-area: 1 / 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    line-height: 16px;
  }

  .lyric .line {
    font-size: 13px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.9);
  }

  .lyric .source {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.5);
  }

  .controls {
    display: flex;
    justify-content: center;
    gap: 22px;
  }

  .controls button {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: #fff;
    cursor: pointer;
  }

  .controls button:hover {
    background: rgba(255, 255, 255, 0.12);
  }

  .controls svg {
    width: 18px;
    height: 18px;
    fill: currentColor;
  }

  /* The lyrics layout's transport sits under the progress bar, centred in the left column. Play is a
     little larger than the skip buttons. */
  .controls.compact {
    align-items: center;
    justify-content: center;
    gap: 14px;
  }

  .controls.compact button {
    width: 44px;
    height: 44px;
  }

  .controls.compact button.play {
    width: 52px;
    height: 52px;
  }

  .controls.compact svg {
    width: 28px;
    height: 28px;
  }

  .controls.compact button.play svg {
    width: 32px;
    height: 32px;
  }

  .empty {
    margin: auto;
    color: rgba(255, 255, 255, 0.55);
  }

  /* Lyrics layout: art, title and transport on the left, three lines on the right. The pager
     stays centred at the bottom. The body is 224 px tall, less the 14 px safe zone, the 8 px gap
     above the pager and the 24 px pager row, which leaves 178 px for the columns. The side
     insets match the expanded panel's 16 px. */
  /* The body clips at its own box. The fit keeps every line inside it, so this only matters for a line
     longer than the largest notch can hold at the text floor. Such a line is cut at the notch's edge
     and never drawn outside it. */
  .wide {
    display: flex;
    flex-direction: column;
    box-sizing: border-box;
    padding: 14px 16px 0;
    overflow: hidden;
  }

  .wide-body {
    display: flex;
    flex: 1;
    gap: 20px;
    min-height: 0;
    margin-bottom: 8px;
  }

  /* The left column is 250 px: art and text (250 less 88 px art and 12 px gap), the 14 px progress
     row, then the transport row of 52 px, with 10 px gaps: 174 px in all. The column is centred in
     the 178 px body, so it has 2 px above and below. */
  .side {
    display: flex;
    flex: none;
    flex-direction: column;
    justify-content: center;
    gap: 10px;
    width: 250px;
  }

  .now-wide {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }

  /* The title and the artist fill the art's 88 px: a two-line title (2 x 28), an 8 px gap and the
     artist's 24 px line. The title sits on the art's top edge and the artist on its bottom edge,
     so a one-line title still spans the same height. */
  .now-wide .meta {
    align-self: stretch;
    justify-content: space-between;
    gap: 8px;
  }

  /* Two lines at most, and a word too long for the column breaks by character, so the second line
     ends in an ellipsis instead of the word running out of the box. The column is fixed at 150 px,
     so a title never widens the notch (see DESIGN.md, section 3.3). */
  .heading {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    overflow-wrap: anywhere;
    font-size: 24px;
    font-weight: 600;
    line-height: 28px;
  }

  /* The lyrics layout's artist is one line, larger than in the panel. Only this layout changes. */
  .wide .artist {
    font-size: 17px;
    line-height: 24px;
  }

  /* The stack is centred in the body, and every line shows whole: a line that is too long for the
     width wraps (the notch is sized for it, see lyricsFit.ts). --ls scales the text, and is 1 unless
     a track's lines are too long even for the largest notch. Each block is absolute, so an outgoing
     block fades over the new one. */
  .stack-wrap {
    position: relative;
    flex: 1;
    min-width: 0;
  }

  .stack {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 8px;
  }

  .stack p {
    margin: 0;
  }

  /* Weight 400 is what lib/lyricsFit.ts measures. The content's 500 would be wider, and a row that
     wraps one word more than the fit counted pushes the stack past the notch. */
  .stack .ctx {
    min-height: calc(26px * var(--ls, 1));
    overflow-wrap: anywhere;
    font-size: calc(21px * var(--ls, 1));
    font-weight: 400;
    line-height: calc(26px * var(--ls, 1));
    color: rgba(255, 255, 255, 0.4);
  }

  /* Room for two lines of 32 px at least, so a short line sits centred in the same 64 px box and the
     block does not jump. A longer line makes the box taller. */
  .stack .cur {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-height: calc(64px * var(--ls, 1));
    overflow-wrap: anywhere;
    font-size: calc(27px * var(--ls, 1));
    font-weight: 600;
    line-height: calc(32px * var(--ls, 1));
    color: #fff;
  }

  /* The live activity. The ring is a pair of half discs: each disc turns inside its half, and the
     clip shows only its own semicircle. Only transform changes, once a second, so the browser
     runs the sweep on the compositor. Reduced motion jumps to each new angle. */
  /* Three columns: the ring on the left, the word and session dots in the middle, the digits on
     the right. The outer columns are 52 px, which holds the longest clock (1:02:33, about 50 px).
     They are the same width, so the middle is centred and 92 px wide: enough for "Long break" (59 px),
     a 6 px gap and four dots (25 px). */
  .activity-row {
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr) 52px;
    align-items: center;
    padding: 0 12px;
  }

  .activity-row .lead {
    display: flex;
    align-items: center;
    justify-self: start;
  }

  .activity-row .ring {
    width: 16px;
    height: 16px;
  }

  .activity-row .mid {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-width: 0;
  }

  /* The word fades at the right edge if it is too long for the middle column. */
  .activity-row .word {
    min-width: 0;
    overflow: hidden;
    font-size: 11px;
    font-weight: 500;
    line-height: 16px;
    white-space: nowrap;
    text-overflow: ellipsis;
    color: rgba(255, 255, 255, 0.6);
  }

  .activity-row .cycle {
    display: flex;
    flex: none;
    gap: 3px;
  }

  /* An unfilled session is the accent at low opacity. Only opacity animates. */
  .activity-row .cdot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--c);
    opacity: 0.25;
    transition: opacity 220ms ease;
  }

  .activity-row .cdot.on {
    opacity: 1;
  }

  .activity-row .clock-slot {
    display: flex;
    align-items: center;
    justify-self: end;
    font-size: 13px;
    font-weight: 600;
    transition: opacity 300ms ease;
  }

  .activity-row.paused .clock-slot {
    opacity: 0.5;
  }

  .stopwatch-glyph {
    flex: none;
    width: 16px;
    height: 16px;
    fill: none;
    stroke: var(--c);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .ring {
    position: relative;
    display: block;
    flex: none;
    border-radius: 50%;
  }

  .ring .track,
  .ring .core {
    position: absolute;
    inset: 0;
    border-radius: 50%;
  }

  .ring .track {
    background: rgba(255, 255, 255, 0.16);
  }

  /* The core is the pill's black, so only the 2 px band shows the ring. */
  .ring .core {
    inset: 2px;
    background: #000;
  }

  .ring .half {
    position: absolute;
    top: 0;
    width: 50%;
    height: 100%;
    overflow: hidden;
  }

  .ring .half.right {
    left: 50%;
  }

  .ring .half.left {
    left: 0;
  }

  .ring .disc {
    position: absolute;
    top: 0;
    width: 200%;
    height: 100%;
    border-radius: 50%;
    background: linear-gradient(var(--from), var(--to));
    transition: transform 1s linear;
  }

  .ring .half.right .disc {
    left: -100%;
    clip-path: inset(0 0 0 50%);
    transform: rotate(var(--right));
  }

  .ring .half.left .disc {
    left: 0;
    clip-path: inset(0 50% 0 0);
    transform: rotate(var(--left));
  }

  /* The finished card. Its glow sits under the outline, so only the halo shows past the edge. */
  .alert-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px;
  }

  .alert-ring {
    position: relative;
    flex: none;
    width: 22px;
    height: 22px;
  }

  .alert-ring .ring {
    width: 22px;
    height: 22px;
  }

  .alert-ring .ripple {
    position: absolute;
    inset: 0;
    border: 2px solid var(--c);
    border-radius: 50%;
    opacity: 0;
    pointer-events: none;
  }

  .glow {
    position: absolute;
    inset: 6px 4px 0;
    border-radius: 26px;
    box-shadow: 0 0 26px 6px var(--c);
    opacity: 0;
    pointer-events: none;
  }

  .shape.alert.on .glow {
    animation: glow 2400ms ease-out forwards;
  }

  @keyframes glow {
    0% {
      opacity: 0;
    }
    14% {
      opacity: 0.6;
    }
    100% {
      opacity: 0.16;
    }
  }

  .alert-copy {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .alert-title {
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
  }

  .alert-sub {
    overflow: hidden;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .alert-actions {
    display: flex;
    flex: none;
    gap: 6px;
  }

  .alert .pill {
    height: 26px;
    padding: 0 11px;
    border: 0;
    border-radius: 13px;
    background: rgba(255, 255, 255, 0.14);
    color: #fff;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .alert .pill.strong {
    background: linear-gradient(90deg, var(--c), var(--c2));
    color: #000;
  }

  .alert .pill:focus-visible {
    outline: 1px solid rgba(255, 255, 255, 0.7);
  }

  @media (prefers-reduced-motion: reduce) {
    .island,
    .shape,
    .frame,
    .page {
      transform: none !important;
      transition: opacity 150ms ease;
    }

    .bars span {
      transform: scaleY(0.6);
    }

    .shape.active.on .bars span {
      animation: none;
    }

    /* The bar still shows the level, but it jumps to it rather than gliding. */
    .hud-fill,
    .battery-glyph .glyph-fill {
      transition: none;
    }

    .ring .disc,
    .activity-row .clock-slot {
      transition: none;
    }

    .shape.alert.on .glow {
      animation: none;
      opacity: 0.35;
    }

    .alert-ring .ripple {
      display: none;
    }
  }
</style>
