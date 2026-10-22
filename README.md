# Notchy

A Dynamic Island-style notch overlay for Windows. Tauri 2 (Rust) + Svelte 5 + TypeScript.

- **Idle**: small black pill at the top center of the primary monitor.
- **Active** (media playing): the pill widens, showing album art and equalizer bars. The bars follow the system's audio output while this pill is on screen (see Audio levels below), and animate on a timer otherwise.
- **Expanded** (hover): two pages, switched by the dots, a wheel turn, or a horizontal swipe. The **Media** page shows album art, title, artist, the current synced lyric line (or "Playing from <app>" when there are no lyrics), and previous / play-pause / next controls. The **Controls** page has volume and brightness sliders and toggle tiles (Wi-Fi, Bluetooth, airplane, mute, keep awake, lock), driven by the controls backend. Opening the panel picks the page: the last one if it was left within 8 s, otherwise Media when something is playing, else Controls. The page list lives in `src/stores/pages.ts`. On the Media page, when the track has synced lyrics, hovering widens the panel into a lyrics layout: circular album art, large transport controls centred under it, and three lyric lines (the current one up to two lines) over a tint taken from the album art.
- **HUD** (the system volume, mute or panel brightness changed): the pill widens into a slim bar for about 1.6 s, then gives way again. Each new change restarts that time. Hover still wins, and a fullscreen app still hides the notch (see Volume and brightness HUD below).
- **Tucked** (a window sits under the notch): shrinks to a thin sliver. Hover it for 250 ms to open it.
- **Hidden** (a fullscreen app owns the monitor): fully invisible and non-interactive.

## Prerequisites

- Windows 10/11 with the WebView2 runtime (preinstalled on Windows 11)
- [Node.js](https://nodejs.org) 20+
- [Rust](https://rustup.rs) (stable, `x86_64-pc-windows-msvc`)
- Visual Studio Build Tools with the "Desktop development with C++" workload

## Run

```bash
npm install
npm run tauri dev
```

Other commands: `npm run tauri build` (release bundle), `npm run check` (Svelte/TS), `cargo test` in `src-tauri`.

## Architecture

| Layer | File | Job |
| --- | --- | --- |
| Overlay shell | `src-tauri/src/window.rs` | Frameless transparent window, `WS_EX_TOOLWINDOW`/`WS_EX_NOACTIVATE` (guarded by a window subclass so Tauri can't reset them), top-center placement, cursor-poll click-through |
| State machine | `src-tauri/src/state.rs` | `Idle` / `Active` / `Expanded` / `Tucked` / `Hidden`, emitted as `island://state` |
| Modules | `src-tauri/src/modules/` | Isolated feature modules; they emit events and never touch the window. `media.rs` reports playback, `lyrics.rs` fetches synced lyrics for the current track, `context.rs` reports screen presence |
| Frontend | `src/` | Renders whatever state it receives; animates only `transform` and `opacity` |

**Click-through:** the window is bigger than the pill and ignores the mouse by default. A thread polls `GetCursorPos` at ~60 Hz, and only on entering or leaving the pill bounds does it toggle click-through and emit `island://hover`.

**Media:** `modules/media.rs` uses the Windows media transport controls (SMTC). It is event-driven (session list, properties, playback and timeline changes), with no polling, and emits `media://update`. It tracks every media session and shows the first one that is playing, else the current one. Playback controls target that same session. The position is sent with the time it was sampled, so the frontend extrapolates while playing.

**Lyrics:** `modules/lyrics.rs` listens for `media://update`. For each new track it fetches synced lyrics from [LRCLIB](https://lrclib.net) on a worker thread, and emits `lyrics://update`. The last 20 results are cached in memory, including "no lyrics", so replays do not refetch. Before the query, titles are cleaned of video decoration such as "(Official Video)" and "(Lyrics)", and "Artist - Song" titles are split.

**Audio levels:** while the compact Active pill is on screen and media is playing, the frontend calls `audio_set_active(true)`, and the backend streams four bar levels on `audio://levels` at about 30 Hz. Every other state turns analysis off, after a 150 ms debounce. If the command is missing, capture fails, or no frame arrives for 400 ms, the bars fall back to the CSS animation. Reduced motion keeps the bars static and never starts analysis.

**Privacy:** lyrics lookups send the cleaned track title and artist to `lrclib.net`, plus the album and duration when the app reports them. A track is sent only the first time it plays, while it stays in the cache. Nothing else leaves the machine. Turn lyrics off in Settings and nothing is sent.

**Audio-reactive bars:** `modules/audio.rs` (with `audio/analysis.rs`) drives the equalizer bars in the compact pill. `audio_set_active` starts one capture thread only while the bars are shown. It records the default output through WASAPI loopback (what the speakers play, not the microphone), mixes it to mono, and runs a 1024-sample Hann-windowed FFT about 30 times a second. Four log-spaced bands (40 to 250 Hz, 250 Hz to 1 kHz, 1 to 4 kHz, 4 to 12 kHz) map through dB to levels from 0 to 1, which are smoothed and sent as `audio://levels` only when they change. Privacy: audio is analysed locally, only four numbers per frame leave the capture thread, and nothing is stored or sent anywhere. With no output device, or no sound, the levels are zero, and a failed capture emits nothing.

**Screen awareness:** `modules/context.rs` installs WinEvent hooks for foreground changes, location changes, minimize, show/hide and cloak. Hooks only post a message; a worker coalesces bursts (50 ms) and classifies the foreground window on the primary monitor:

- **Fullscreen** (`Hidden`): the window covers the whole monitor, has no title bar, and is not maximized. Only non-maximized windows hide the island. A maximized window always tucks instead, even when it is frameless and its frame reaches every monitor edge (auto-hidden taskbar). Electron and Chromium un-maximize before entering real fullscreen (F11, HTML fullscreen), and games and players are not maximized. The system's fullscreen signal (`SHQueryUserNotificationState`) only counts for a monitor-covering, non-maximized window, because it is machine-wide.
- **Covered** (`Tucked`): the window's visible frame overlaps the notch zone, which is the Active pill's width plus a margin, from the top edge down past the pill, scaled by DPI. This applies to maximized windows too.
- The shell (desktop, taskbars, Start, Alt+Tab, Task View), this app's own windows, and windows minimized, cloaked or on other monitors are ignored.

Each recompute also runs a settle step. Fullscreen commits only after it has read true for 150 ms, so a transient frame cannot hide the island; leaving fullscreen and every other change commit at once. Each event burst also schedules rechecks at +120 ms and +400 ms, which catch a reading taken mid-transition when no further event arrives.

**Debugging:** set `NOTCHY_DEBUG=1` before launching (for example, `set NOTCHY_DEBUG=1` then `npm run tauri dev`). Every recompute logs its trigger, the foreground window (hwnd, class, title, frame, style, cloaked, notification state), the monitor rect, the raw result and whether it was committed, to stderr and to `%TEMP%\notchy-debug.log`. That file restarts once it passes 1 MB. With the variable unset, the only cost is one cached bool.

A tucked sliver only expands after the cursor has stayed on it for 250 ms (`TUCKED_HOVER_DWELL` in `window.rs`), so sweeping across an app's tab strip does not pop the island open. Leaving is immediate.

**Volume and brightness HUD:** `modules/hud.rs` shows a slim HUD when the system volume, mute or panel brightness changes. It emits `hud://update` with `{kind, level, muted}` and holds the `hud` island state (`IslandState::Hud`) for 1.6 s after the last change, so each new change restarts that time. Volume is event-driven: `modules/hud/volume.rs` registers an `IAudioEndpointVolumeCallback` on the default output, so every change is seen whatever caused it (volume keys, Settings, another app). An `IMMNotificationClient` re-attaches it when the default output changes. Brightness is event-driven too: `modules/hud/brightness.rs` watches WMI's `WmiMonitorBrightnessEvent` in `root\WMI`, which the laptop panel's driver raises on a change. Nothing polls. The scheduler drops repeats, coalesces a key-repeat burst into updates at most about 33 ms apart (the newest value always lands), and keeps the hold running from the newest change. Precedence: fullscreen hides the HUD, hover wins over it (the panel opens as usual), and the HUD shows over Tucked and Active. Changes made by the panel's own sliders are ignored: volume writes carry an event context GUID that the callback drops, and brightness writes are absorbed for 2 s. The setting is `hud` in `config.json` (Settings > General).

The HUD does not hide the native Windows volume or brightness OSD. Windows draws its own flyout for the volume and brightness keys, and Notchy has no safe, documented hook to suppress it, so both appear. Brightness only shows where the panel's driver raises the WMI brightness event. External monitors (DDC/CI) raise no event, so they show no HUD.

## Quick controls

`modules/controls.rs` backs the notch's quick settings. Each control is a `Control` impl in `modules/controls/`, listed in the `CONTROLS` registry. Nothing polls: the frontend calls `controls_list` when the notch opens, `controls_set` for sliders and toggles, and `controls_press` for buttons. Unavailable controls come back with `supported: false` and a reason in `detail`.

Ids: `volume`, `mute`, `mic`, `brightness`, `wifi`, `bluetooth`, `airplane`, `lock`, `screenshot`, `awake`, `darkmode`. Airplane mode is emulated: it turns every radio off and remembers which were on, since Windows has no public airplane-mode API. Keep-awake holds a thread for as long as it is on, and resets on exit.

## Settings and tray

The tray icon opens **Settings** on left-click (or from its menu). Settings is a second window, built on first use and hidden, not closed, when you close it. The tray menu also has **Quit Notchy**, the only way to exit the app.

Settings holds the notch pages and their order, the quick controls and their order (at most seven tiles, since sliders sit above the row), show lyrics, the volume and brightness HUD, confirm before locking, and launch at login. Every change saves at once and applies live.

The file is `config.json` in the app config directory (`%APPDATA%\com.notchy.app\config.json`). `config.rs` owns it: a missing or corrupt file gives the defaults, and saves are written through a temp file and renamed. Launch at login is the `Notchy` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

## Fonts

Settings > Appearance > Font chooses the typeface for the notch and the settings window. The choice saves at once and applies live, and the notch keeps its size. The choices are:

- **SF Pro (if installed)**: uses Apple's SF only when it is already installed on the machine, and otherwise falls back to Inter.
- **Inter** (default), **Manrope**, **DM Sans**, **Space Grotesk**, **Playfair Display** (serif), and **System (Segoe UI Variable)**, which is the old behaviour.

Apple's SF is **not bundled**. Its licence limits use to Apple platforms, so Notchy ships none of its files. The other fonts come from the `@fontsource-variable/*` packages under the SIL Open Font License 1.1. Only the latin subset of each is bundled (`src/fonts.css`), so the fonts work offline. The choice is `font` in `config.json`, one of the ids in `FONTS` in `config.rs`.

## Adding a module

1. Create `src-tauri/src/modules/yours.rs` and implement the `Module` trait from `modules/mod.rs`.
2. Add it to `start_all` in `modules/mod.rs`.
3. Emit your own events; if it should affect the pill, feed an input into `IslandStateMachine` in `state.rs`.
4. Listen for the event from a store in `src/stores/`.

## Known limits (v0.1)

- Pill sizes are defined in both `state.rs` (hit-testing) and `Island.svelte` (rendering). Keep them in sync. The window is 720 x 240 (`tauri.conf.json` and `window.rs`), which holds the largest pill, the 620 x 214 lyrics layout.
- Album art is read once per track; an app that publishes its thumbnail late may show the placeholder.
- Only the primary monitor is supported.
- The volume and brightness HUD follows the default output and the built-in panel. External monitors show no brightness HUD, and the native Windows OSD still appears alongside it.
