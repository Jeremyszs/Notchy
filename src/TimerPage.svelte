<script lang="ts">
  // The Timer page of the expanded panel: a timer with quick presets and a stepper, a stopwatch
  // with laps, and a pomodoro cycle. The backend owns the clock. This page only shows it and sends
  // the buttons' commands.
  import { get } from "svelte/store";
  import { fade } from "svelte/transition";
  import {
    addTime,
    dismissTimer,
    lapTimer,
    pauseTimer,
    resetTimer,
    resumeTimer,
    selectMode,
    startTimer,
    timerState,
    timerView,
  } from "./stores/timer";
  import {
    clamp,
    formatStopwatch,
    MAX_LAPS,
    MAX_TIMER_MINUTES,
    MIN_TIMER_MINUTES,
    minutesOf,
    phaseLabel,
    TIMER_PRESETS_MIN,
    viewAt,
    type TimerMode,
  } from "./lib/timer";
  import RollingClock from "./RollingClock.svelte";

  const MODES: { id: TimerMode; label: string }[] = [
    { id: "timer", label: "Timer" },
    { id: "stopwatch", label: "Stopwatch" },
    { id: "pomodoro", label: "Pomodoro" },
  ];
  const ONE_MINUTE_MS = 60_000;
  // A mode change crossfades the content below the selector. Opacity only, in every motion setting.
  const MODE_FADE = { duration: 150 };

  $: view = $timerView;
  // A run in progress, paused or waiting to be dismissed, locks the mode and the goal.
  $: busy = view.active;
  $: minutes = minutesOf(view.totalMs);

  function choose(mode: TimerMode) {
    if (busy || mode === view.mode) return;
    void selectMode(mode);
  }

  function setMinutes(next: number) {
    if (busy) return;
    void selectMode("timer", clamp(Math.round(next), MIN_TIMER_MINUTES, MAX_TIMER_MINUTES));
  }

  // The stopwatch's hundredths tick with the frame rate, but only while it runs, and the browser
  // stops the loop when the page is hidden. Everything else on the page updates per second.
  function live(node: HTMLElement) {
    let frame = 0;
    const paint = () => {
      frame = 0;
      const { snapshot, receivedMs } = get(timerState);
      const now = viewAt(snapshot, Date.now(), receivedMs);
      node.textContent = formatStopwatch(now.elapsedMs);
      if (now.running) frame = requestAnimationFrame(paint);
    };
    const unsubscribe = timerState.subscribe(() => {
      cancelAnimationFrame(frame);
      paint();
    });
    return {
      destroy() {
        cancelAnimationFrame(frame);
        unsubscribe();
      },
    };
  }
</script>

<div class="timer" style="--c: {view.palette.from}; --c2: {view.palette.to}">
  <!-- Docked: the selector is the first row of the page, and no mode or state moves it. -->
  <div class="tabs" role="group" aria-label="Clock">
    {#each MODES as item (item.id)}
      <button
        type="button"
        aria-pressed={view.mode === item.id}
        disabled={busy && view.mode !== item.id}
        onclick={() => choose(item.id)}
      >
        {item.label}
      </button>
    {/each}
  </div>

  <!-- Everything below the selector gets the rest of the page box. The body for the current mode
       is absolutely placed in that area and centred in it, so the outgoing and incoming bodies
       crossfade over the same space and neither one changes the layout. -->
  <div class="lower">
    {#key view.mode}
      <div class="body" in:fade={MODE_FADE} out:fade={MODE_FADE}>
        {#if view.mode === "stopwatch"}
          <div class="digits" class:paused={view.status === "paused"} use:live></div>
        {:else}
          <div class="digits" class:paused={view.status === "paused"}>
            <RollingClock text={view.label} />
          </div>
        {/if}

        {#if view.mode === "pomodoro"}
          <div class="cycle">
            <span>{phaseLabel(view)}</span>
            <span class="dots" aria-label="{view.pomodoro.completed} of {view.pomodoro.of} focus sessions done">
              {#each Array(view.pomodoro.of) as _, n (n)}
                <span class="dot" class:on={n < view.pomodoro.completed}></span>
              {/each}
            </span>
          </div>
        {/if}

        {#if view.mode === "timer"}
          <div class="presets" role="group" aria-label="Presets">
            {#each TIMER_PRESETS_MIN as preset (preset)}
              <button
                type="button"
                class="chip"
                class:on={!busy && minutes === preset}
                disabled={busy}
                onclick={() => setMinutes(preset)}
              >
                {preset}
              </button>
            {/each}
          </div>
        {/if}

        <div class="actions">
          {#if view.status === "done"}
            <button type="button" class="pill" onclick={() => addTime(ONE_MINUTE_MS)}>+1 min</button>
            <button type="button" class="pill strong" onclick={dismissTimer}>Dismiss</button>
          {:else if view.status === "running"}
            <button type="button" class="pill" onclick={pauseTimer}>Pause</button>
            {#if view.mode === "stopwatch"}
              <button
                type="button"
                class="pill"
                disabled={view.laps.length >= MAX_LAPS}
                onclick={lapTimer}
              >
                Lap
              </button>
            {/if}
            <button type="button" class="pill quiet" onclick={resetTimer}>Reset</button>
          {:else if view.status === "paused"}
            <button type="button" class="pill strong" onclick={resumeTimer}>Resume</button>
            <button type="button" class="pill quiet" onclick={resetTimer}>Reset</button>
          {:else if view.mode === "timer"}
            <div class="stepper">
              <button
                type="button"
                class="step"
                aria-label="One minute less"
                disabled={minutes <= MIN_TIMER_MINUTES}
                onclick={() => setMinutes(minutes - 1)}
              >
                −
              </button>
              <span class="minutes">{minutes} min</span>
              <button
                type="button"
                class="step"
                aria-label="One minute more"
                disabled={minutes >= MAX_TIMER_MINUTES}
                onclick={() => setMinutes(minutes + 1)}
              >
                +
              </button>
            </div>
            <button type="button" class="pill strong" onclick={startTimer}>Start</button>
          {:else}
            <button type="button" class="pill strong" onclick={startTimer}>Start</button>
          {/if}
        </div>

        {#if view.mode === "stopwatch" && view.laps.length > 0}
          <ol class="laps" aria-label="Laps">
            {#each view.laps.slice().reverse() as lap, i (view.laps.length - i)}
              <li>
                <span class="name">Lap {view.laps.length - i}</span>
                <span>{formatStopwatch(lap.totalMs)}</span>
                <span class="split">+{formatStopwatch(lap.splitMs)}</span>
              </li>
            {/each}
          </ol>
        {/if}
      </div>
    {/key}
  </div>
</div>

<style>
  /* The selector is docked at the top of the page box. Only the area below it adapts to the mode,
     and the rows in that area are centred in it, so the gap above equals the gap below. */
  .timer {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-height: 0;
    height: 100%;
    color: #fff;
  }

  .tabs {
    flex: none;
    display: flex;
    align-self: center;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.08);
  }

  .tabs button {
    height: 20px;
    padding: 0 10px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: rgba(255, 255, 255, 0.6);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    cursor: pointer;
  }

  .tabs button[aria-pressed="true"] {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }

  .tabs button:disabled {
    cursor: default;
    opacity: 0.35;
  }

  /* The area below the selector: whatever height the page box leaves. */
  .lower {
    position: relative;
    flex: 1 1 0;
    min-height: 0;
  }

  /* One mode's content, filling the area and centring its rows in it. */
  .body {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 6px;
    min-height: 0;
  }

  .body > * {
    flex: none;
  }

  /* Fixed height, so a change of digits or mode never moves the rows below. */
  .digits {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 40px;
    font-size: 36px;
    font-weight: 600;
    line-height: 40px;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
    transition: opacity 320ms ease;
  }

  .digits.paused {
    opacity: 0.5;
  }

  .cycle {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    height: 16px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.6);
  }

  .dots {
    display: flex;
    gap: 5px;
  }

  /* An unfilled session is the accent at low opacity. Only opacity animates. */
  .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--c);
    opacity: 0.25;
    transition: opacity 220ms ease;
  }

  .dot.on {
    opacity: 1;
  }

  .presets {
    display: flex;
    justify-content: center;
    gap: 6px;
    height: 24px;
  }

  .chip {
    min-width: 44px;
    height: 24px;
    padding: 0 8px;
    border: 0;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.1);
    color: #fff;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .chip.on {
    background: var(--c);
    color: #000;
  }

  .chip:disabled {
    cursor: default;
    opacity: 0.35;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 30px;
  }

  .pill {
    height: 28px;
    padding: 0 14px;
    border: 0;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.14);
    color: #fff;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .pill.strong {
    background: linear-gradient(90deg, var(--c), var(--c2));
    color: #000;
  }

  .pill.quiet {
    background: transparent;
    color: rgba(255, 255, 255, 0.7);
  }

  .pill:disabled {
    cursor: default;
    opacity: 0.35;
  }

  .stepper {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .step {
    width: 28px;
    height: 28px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.14);
    color: #fff;
    font: inherit;
    font-size: 16px;
    line-height: 28px;
    cursor: pointer;
  }

  .step:disabled {
    cursor: default;
    opacity: 0.35;
  }

  .minutes {
    min-width: 56px;
    text-align: center;
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  /* The laps take the height the rows above leave. Rows are 16 px, and a longer list scrolls
     inside this box, newest first. Below the rows the list is only as tall as its laps, so a short
     list stays centred with the other rows. */
  .laps {
    flex: 0 1 auto;
    min-height: 0;
    margin: 0;
    padding: 0 4px 0 0;
    overflow-y: auto;
    list-style: none;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.2) transparent;
  }

  .laps li {
    display: flex;
    gap: 8px;
    height: 16px;
    line-height: 16px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.85);
  }

  .laps .name {
    flex: 1;
    color: rgba(255, 255, 255, 0.55);
  }

  .laps .split {
    color: rgba(255, 255, 255, 0.5);
  }

  @media (prefers-reduced-motion: reduce) {
    .digits,
    .dot {
      transition: none;
    }
  }
</style>
