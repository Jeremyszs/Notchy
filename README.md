# Notchy

A Dynamic Island-style notch overlay for Windows. Tauri 2 (Rust) + Svelte 5 + TypeScript.

> **UI work:** read [DESIGN.md](DESIGN.md) before you touch the notch, its pages, sizes or motion. It holds the size table, the layout and centring rules, the type and colour values, and the checklist for UI changes.

- **Idle**: small black pill at the top center of the primary monitor, with the time (HH:MM, dim) in the middle. Settings turns the time off (`idleClock`).
- **Active** (media playing): the pill widens, showing album art and equalizer bars. The bars follow the system's audio output while this pill is on screen (see Audio levels below), and animate on a timer otherwise.
- **Expanded** (hover): pages, switched by the dots, a wheel turn, or a horizontal swipe. The **Calendar** page shows the month, the selected day's notes, and an **Add note** button that opens Settings on that day (see Calendar below). The **Stats** page (see System stats below) shows live system stats. The **Media** page shows album art, title, artist, the current synced lyric line (or "Playing from <app>" when there are no lyrics), and previous / play-pause / next controls. Under the artist, a slim progress bar shows the elapsed and total time, and a click, a drag or the arrow keys seek (see Media below). The **Controls** page has volume and brightness sliders and toggle tiles (Wi-Fi, Bluetooth, airplane, mute, keep awake, lock), driven by the controls backend. Opening the panel picks the page: the last one if it was left within 8 s, otherwise Media when something is playing, else Controls. The page list lives in `src/stores/pages.ts`. On the Media page, when the track has synced lyrics, hovering widens the panel into a lyrics layout: circular album art with the title and artist beside it, the progress bar and transport controls under them, and three lyric lines over a tint taken from the album art. The layout is 620 x 224 at rest. When a line is too long for that, it widens to at most 700 x 240 (wider first, then taller), and the text shrinks only as a last step, so every line shows whole, with no ellipsis. The title wraps to two lines and ends in an ellipsis, and it never widens the notch (see DESIGN.md section 3.3).
- **HUD** (the system volume, mute or panel brightness changed): the pill widens into a slim bar for about 1.6 s, then gives way again. Each new change restarts that time. Hover still wins, and a fullscreen app still hides the notch (see Volume and brightness HUD below).
- **Notification** (another app shows a Windows toast): a slim pill for 5 s with the app's initial, the title and the start of the body. Newer ones wait their turn. Off in Settings, or per app. It needs Windows' notification access (see Notifications below).
- **Battery** (the charger is plugged in or out, the battery is full, or it is low): a slim pill for 3 s with a battery glyph, the level, and the words. A low warning shows for 5 s, amber at 20% and red at 10%. A desktop with no battery shows nothing (see Battery alerts below).
- **Calendar reminder** (one of your own events is about to start, or has started, or an all-day event's morning): a slim pill for 6 s (8 s when the event starts) with a calendar glyph, the title and "in 10 min" or "now". It takes its turn behind a battery pill and ahead of another app's notification (see Calendar below).
- **Activity** (a timer is running, paused, or finished and waiting to be dismissed): the compact pill shows a ring on the left and the time on the right, with the word in the middle (Timer, Stopwatch, Focus, Break, Long break, Paused or Done). A pomodoro also shows one dot per focus session. It takes the place of the media title and bars. Hover expands it as usual.
- **Alert** (a countdown just reached zero): a card for about 4 s, with the copy, **+1 min** and **Dismiss**. It holds even under the pointer, so its buttons stay where they are.
- **Tucked** (a window sits under the notch): shrinks to a thin sliver. Hover it for 250 ms to open it. A covering window also tucks a live activity.
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
| Modules | `src-tauri/src/modules/` | Isolated feature modules; they emit events and never touch the window. `media.rs` reports playback, `lyrics.rs` fetches synced lyrics for the current track, `context.rs` reports screen presence, `notifications.rs` shows other apps' toasts, `battery.rs` shows charger and low-battery pills, `stats.rs` samples the system for the Stats page, `calendar.rs` reminds of the user's own events, `hotkeys.rs` registers the global hotkeys |
| Frontend | `src/` | Renders whatever state it receives; animates only `transform` and `opacity` |

**Click-through:** the window is bigger than the pill and ignores the mouse by default. A thread polls `GetCursorPos` at ~60 Hz, and only on entering or leaving the pill bounds does it toggle click-through and emit `island://hover`.

**Media:** `modules/media.rs` uses the Windows media transport controls (SMTC). It is event-driven (session list, properties, playback and timeline changes), with no polling, and emits `media://update`. It tracks every media session and shows the first one that is playing, else the current one. Playback controls target that same session. The position is sent with the time it was sampled, so the frontend extrapolates while playing. `media_seek` moves the current session to a position in milliseconds, clamped to the track's length, and the new position arrives as an ordinary update. The progress bar is hidden when the length is unknown, as it is for a live stream. The clock under it runs only while the panel shows the Media page and a track plays, and a seek shows its target until the next update.

**Lyrics:** `modules/lyrics.rs` listens for `media://update`. For each new track it fetches synced lyrics from [LRCLIB](https://lrclib.net) on a worker thread, and emits `lyrics://update`. The last 20 results are cached in memory, including "no lyrics", so replays do not refetch. Before the query, titles are cleaned of video decoration such as "(Official Video)" and "(Lyrics)", and "Artist - Song" titles are split.

**Audio levels:** while the compact Active pill is on screen and media is playing, the frontend calls `audio_set_active(true)`, and the backend streams four bar levels on `audio://levels` at about 30 Hz. Every other state turns analysis off, after a 150 ms debounce. If the command is missing, capture fails, or no frame arrives for 400 ms, the bars fall back to the CSS animation. Reduced motion keeps the bars static and never starts analysis.

**Privacy:** lyrics lookups send the cleaned track title and artist to `lrclib.net`, plus the album and duration when the app reports them. A track is sent only the first time it plays, while it stays in the cache. Nothing else leaves the machine. Turn lyrics off in Settings and nothing is sent. Notification text is read and shown on this PC only, and never leaves it.

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

## Timer, stopwatch and pomodoro

The **Timer** page (page id `timer`) runs three clocks: a countdown with quick presets (1, 5, 10, 25 min) and a stepper, a stopwatch with laps (at most 20), and a pomodoro cycle (25 min focus, 5 min short break, 15 min long break after four focus sessions, phases advancing on their own). One clock runs at a time, and the mode cannot change while it runs.

`modules/timer.rs` owns the clock, so a running timer survives page changes, a hidden notch and a closed Settings window. `Engine` is pure: every method takes the instant to act at, and the tests pass fixed times. A worker thread sleeps on a condition variable until the next deadline (a countdown reaching zero, or the alert giving way), so nothing polls. Commands change the engine under the same lock and wake the worker. The backend emits `timer://update` with a snapshot (including `endsAtMs`, so the frontend counts down between updates), `timer://finished` when a countdown ends, and the island state (`activity`, `alert`). Commands: `timer_snapshot`, `timer_select`, `timer_start`, `timer_pause`, `timer_resume`, `timer_reset`, `timer_add_ms`, `timer_lap`, `timer_dismiss`.

The fixed rules, which the tests pin down: a finished timer stays in the pill until dismissed, and "+1 min" starts it again with that minute left. A late tick does not drift a pomodoro, because each phase starts at the moment the previous one ended. Nothing is persisted except the settings below.

Precedence (highest first): fullscreen hides it; an alert shows over hover; hover expands; the HUD; a battery pill; a notification; a covering window tucks it; the activity; media. When a timer runs and media plays, the timer owns the compact pill.

## Notifications

`modules/notifications.rs` shows other apps' Windows toasts in the notch. A poller reads the user notification listener (`UserNotificationListener`) once every 1.5 s, and only while the feature is on. The listener's change event fails for unpackaged apps, so nothing is subscribed. The WinRT calls live in `modules/notifications/listener.rs`. A `Tracker` decides which toasts are new, and a `Queue` shows them one at a time for 5 s each. Both are pure, and the tests pin their rules down.

- **The first poll is silent.** When the feature turns on, or the app starts, the toasts already on screen are recorded and not shown. Turning it on never floods the notch.
- **One at a time.** Toasts show oldest first. A toast that arrives while one is up waits its turn. At most five wait, and beyond that the oldest waiting one is dropped.
- **Precedence.** Fullscreen hides it. A timer alert and hover both outrank it, and so does the HUD. It sits under the HUD so that a volume change is never hidden by a message. Like the HUD, it shows over a covered notch.
- **Text.** The first line is the title and the rest, joined, is the body. Whitespace is collapsed and repeated lines dropped. The title is cut at 80 characters and the body at 160, and the pill cuts to one line with an ellipsis.
- **Access.** Windows must allow it: Settings > Privacy & security > Notifications > "Let apps access your notifications". Notchy asks Windows for access once each time the feature turns on, when Windows has not decided yet. Settings shows Allowed, Denied or Unspecified. It never opens Windows settings for you.

Settings > Notifications holds the switch, the access line and a switch for each app seen so far. An app that is turned off is listed in `notifications.deniedApps` in `config.json`, by its AppUserModelId. The apps seen so far are saved to `notification-apps.json` beside it, with only ids and names, so the list survives a restart. Notification text is never saved or logged.

Limits: a toast appears up to about 1.5 s after it arrives. A toast that is cleared before the next poll is missed. The pill has no click action and no icon, only the app's initial. Focus assist (Do not disturb) is not checked, so a toast that Windows holds back can still show if it is still in the notification list. Only toasts that Windows still lists can be read.

The ring is two half discs, each turning inside its half of the ring, so only `transform` changes, about once a second, and the compositor runs the sweep. The digits roll like an odometer: each changed digit leaves upward while its replacement rises from below over 220 ms (`cubic-bezier(.2,.8,.2,1)`), in fixed tabular columns, and reduced motion keeps a 150 ms crossfade. The last ten seconds beat once a second. The finished ring ripples outward and the card springs open, using the island's own shape easing.

Settings > Timer holds the sound (one system beep when a countdown ends) and the pomodoro lengths (focus 1 to 120 min, breaks 1 to 60 min). They live in `config.json`. Turn the page on in Settings > Notch pages. The page is off in configs written before it existed.

## Battery alerts

`modules/battery.rs` shows a pill when the charger is plugged in or out, when the battery reaches 100% on the charger, and when it falls to 20% or 10% on battery. A poller reads `GetSystemPowerStatus` every 2 s on its own thread. It is a plain Win32 call, so there is no COM or WinRT here. A `Tracker` turns each reading into the events, and a `Slot` holds the pill for its time. Both are pure, and the tests pin their rules down.

- **Charging** shows "Charging" with the level, a green glyph with a bolt, for 3 s. **On battery** (unplugged) shows a white glyph for 3 s. **Fully charged** shows once per charge, green with the bolt.
- **Low battery** warns at 20% (amber) and at 10% (red), for 5 s. Each threshold fires once per discharge. It re-arms when the charger is plugged in, or when the level rises 3 points above it, so a level hovering at the threshold does not warn on every dip.
- The first reading after launch is silent. A level already low at launch counts as shown, so nothing alerts at startup.
- Precedence: a battery pill sits under the HUD and the alert, and over a notification, so a plug event is not held back behind a message. A notification keeps its own timer while the pill is up, and shows once the pill gives way. Hover and fullscreen hide it, as they hide the HUD.
- One pill at a time. A newer event replaces the one on screen and restarts its time.

Settings > Battery holds three switches, all on by default: the feature, the charger and full-charge pills, and the low-battery warnings. They live in `config.json` as `battery` (`enabled`, `chargeAlerts`, `lowBatteryAlerts`). The line under them shows the live state, such as "Battery 82%, charging", or "No battery detected" on a desktop, where the switches are off. Nothing is written to disk.

Limits: a level change shows up to 2 s late, and some drivers report the level in steps. A UPS that reports a battery to Windows counts as one, so its mains loss shows as a discharge.

## System stats

The **Stats** page (page id `stats`) shows CPU use, memory (used of total, GB and %), download and upload speed (KB/s, MB/s or GB/s), and the system drive's use with its free space. Each tile has a sparkline of the last 30 readings. `modules/stats.rs` reads the counters once a second, and only while the expanded panel is open on this page: the frontend tells it with `stats_set_active`, so an idle notch reads nothing. The reads are Win32 calls (`GetSystemTimes`, `GlobalMemoryStatusEx`, `GetIfTable2`, `GetDiskFreeSpaceExW`) that need no admin rights or drivers. The CPU and network rates are smoothed, and the first reading after the page opens shows dashes until the next one. Each reading is sent on `stats://update` and never stored or sent off the machine.

Turn the page on in Settings > Notch pages, and the switch in Settings > Stats (`stats.enabled` in `config.json`) turns sampling off altogether. The page is off in configs written before it existed.

Limits: CPU temperature and GPU use are not shown, since neither is reliably readable without admin rights or a vendor driver. Network speed adds up every interface that is up, except loopback, so a VPN adapter can count the same traffic twice. The disk figure is the system drive only.

## Calendar

`modules/calendar.rs` reminds you of your own events. There is no account and no network: you make the events in **Settings > Calendar**. It is a month grid: click a day to see its notes, and add, edit or delete them there. A note has a title, optional notes, a time (or all day), a repeat (none, daily, weekly, monthly or yearly), and, for a timed note, a length (none, 15 minutes to 2 hours, or a custom number of minutes up to a day) and its reminders. The reminders are either the Settings lead time (the default), up to five picked from 5 min, 10 min, 15 min, 30 min, 1 hour and 1 day, or none beyond the start. Days with notes are coloured in the calendar accent (#FF6A3D), and a repeating note colours every day it falls on. The week starts on the first day of the week for this PC's language, Sunday in en-US and Monday in en-GB. Each timed event is announced before it starts at its reminder times (`leadMinutes` before it, 5, 10, 15, 30 or 60, default 10, unless the note sets its own) and again at its start. An all-day event is announced once, at `allDayHour` (default 9) on its day. The island's **Calendar** page is the same month grid in a compact form, beside the selected day's notes. The arrows change the month, a click selects a day, and the wheel turns pages without changing the month. It has no text inputs, because the island window does not take focus, so **Add note** opens Settings on the selected day with the add form open. The grid fits the 146 px page box (see DESIGN.md, Calendar page).

**Snooze.** After a reminder fires, the most recent one stays snoozable for 30 minutes. The Calendar page then shows a strip under the selected day's notes, with **5 min** and **10 min** buttons. Pressing one shows that reminder again, with the same title, after that many minutes. A newer reminder replaces it, and once a snooze is set the strip goes until the next reminder fires. The snooze is kept in memory only, so a restart drops it. The reminder record in `calendar-reminders.json` is unchanged.

The files are in `src-tauri/src/modules/calendar/`:

| File | Job |
| --- | --- |
| `model.rs` | Events, occurrences and recurrence. Pure date math with no clock, so it is tested with fixed dates. |
| `source.rs` | The `EventSource` trait, and `LocalSource`, the editable events |
| `store.rs` | `events.json` and `calendar-reminders.json` |
| `service.rs` | Merges every source into one sorted list, and sends each change to the source that owns it |
| `reminders.rs` | Which reminders are due, the record of those already shown, and the pill queue. Pure. |

`calendar.rs` holds the scheduler thread and the Tauri commands (`calendar_list`, `calendar_upcoming`, `calendar_between`, `calendar_upsert`, `calendar_delete`, `settings_open_calendar` to open Settings on a day with the add form open, `calendar_take_add_date` to take the pending add day once, `calendar_recent_reminder` for the snoozable reminder, and `calendar_snooze(minutes)`, which accepts 5 or 10). `calendar_between(from, to)` takes two local dates, `YYYY-MM-DD`, both included, and returns every occurrence that overlaps them, in start order, from the same expansion the reminders use. The month grid asks for the six weeks it shows, so the frontend never works out a repeat itself. A range is at most 366 days. The events go to the windows on `calendar://update`, and each reminder goes to the island on `calendar://show`.

- **Storage.** `events.json` sits next to `config.json` and is written through a temp file and a rename. It is versioned (`"version": 1`). A file that cannot be read, or has another version, is moved to `events.json.bak`, and the calendar starts empty, so nothing is lost without a copy. The settings file never holds events, because the Settings window saves that file whole.
- **Times.** Times are local and naive: 14:30 is 14:30 on this PC, stored as `"2026-10-23T14:30"`, not in UTC. A recurring event keeps its wall-clock time across a DST change. A start in the skipped spring-forward hour is treated as the wall-clock time and its reminder may be skipped when the jump makes it overdue by `OVERDUE_LIMIT` or more, while a repeated hour fires once.
- **Repeats.** A monthly event on the 31st falls on the last day of a shorter month and returns to the 31st after it. A yearly event on 29 February falls on 28 February in common years. Each occurrence is computed from the start, so nothing drifts.
- **Each reminder fires once.** Its key is the event, the occurrence and the lead time. A reminder that is up to 5 minutes late still shows, which covers a PC that was asleep. A later one is skipped without a word. The keys already shown are kept in `calendar-reminders.json` (pruned after two days), so a restart never shows one twice.
- **Precedence.** A reminder sits under the battery pill, the HUD and the timer alert, and over a notification, so an app's toast never hides it (`state.rs`). It is one pill at a time, and up to five more wait their turn. It has no click action.
- **Waking.** The scheduler sleeps until the next reminder, and at most a minute, so a PC that slept or a changed clock is caught within a minute. It does not poll.
- **Privacy.** Event titles and notes stay on this PC and are never logged.

**Adding a read-only ICS source later.** The seam is the `EventSource` trait in `source.rs`. It has `id`, `read_only`, `series` (the stored definitions), and `events_between`, whose default expands each series. An ICS source would:

1. Add `calendar/ics.rs` with a struct that implements `EventSource`. `read_only` returns `true`, and `series` returns the feed's events as `CalendarEvent`s. A feed that repeats by RRULE needs `Recurrence` extended with a variant that holds the rule (see `model.rs`). Keep the fetched copy in memory and refresh it on a worker thread, so `series` never waits on the network.
2. Add it to the sources in `start` in `modules/calendar.rs`, with a stable id such as `ics:<hash of the URL>`.

Nothing else changes. The scheduler, the island and the Settings list read only the merged `EventInstance`s, and `upsert` and `remove` refuse a read-only source.

**Config.** `calendar` in `config.json`: `enabled` (on), `leadMinutes` (10), `allDayHour` (9). A calendar page configured before this existed is off until you turn it on in **Settings > Notch pages**. New installs have it on.

**Limits.** Snooze only reaches the most recent reminder, for 30 minutes, and it is not kept across a restart. There is no click action on the pill. Editing a repeating event changes the whole series, and so does deleting it. Per-occurrence edits are not supported. The length and reminders belong to the series, like the rest of the note. There is one local calendar and no ICS yet. The idle clock shows no reminder line. The month grid shows a day's notes as a count and colour only: there is no dot per note. The reminder timing is checked in the unit tests with fixed times, and the pill, the page and the month grid were checked in a browser harness, not in the Tauri window.

## Global hotkeys

Settings > Hotkeys binds four actions to key combinations that work in any app: play or pause, next track, previous track, and pause or resume the timer. The timer key pauses a running timer and resumes a paused one, and does nothing when no timer is running. Opening the notch is not a hotkey.

Hotkeys are off by default, because a global key grab takes its keys away from every other app. Turn the switch on, and each action has its own box, set to `Ctrl+Alt+Space`, `Ctrl+Alt+Right`, `Ctrl+Alt+Left` and `Ctrl+Alt+T` at first. Clear a box to leave that action unbound. The settings are in `config.json` as `hotkeys` (`enabled`, `playPause`, `nextTrack`, `previousTrack` and `timerToggle`). A config written before this existed loads with the defaults and the feature off.

`modules/hotkeys.rs` registers the enabled bindings with the Windows global shortcut plugin, and registers them again whenever the settings change. A handler runs on key press only, and calls the same code as the notch's buttons and the timer's commands. A binding can fail when another app already owns the combination, or when its text is not a key combination. The reason shows under that action in Settings (from `hotkeys_status` and `hotkeys://status`). The other bindings still work, and the app does not stop. Quitting the app unregisters every key.

## Quick controls

`modules/controls.rs` backs the notch's quick settings. Each control is a `Control` impl in `modules/controls/`, listed in the `CONTROLS` registry. Nothing polls: the frontend calls `controls_list` when the notch opens, `controls_set` for sliders and toggles, and `controls_press` for buttons. Unavailable controls come back with `supported: false` and a reason in `detail`.

Ids: `volume`, `mute`, `mic`, `brightness`, `wifi`, `bluetooth`, `airplane`, `lock`, `screenshot`, `awake`, `darkmode`. Airplane mode is emulated: it turns every radio off and remembers which were on, since Windows has no public airplane-mode API. Keep-awake holds a thread for as long as it is on, and resets on exit.

## Settings and tray

The tray icon opens **Settings** on left-click (or from its menu). Settings is a second window, built on first use and hidden, not closed, when you close it. The tray menu also has **Quit Notchy**, the only way to exit the app.

Settings holds the notch pages and their order, the quick controls and their order (at most seven tiles, since sliders sit above the row), show lyrics, show the time when idle, the volume and brightness HUD, confirm before locking, launch at login, the timer's sound and pomodoro lengths, the notifications switch with its app list, the battery alerts with their live status line, the calendar with its lead time and event editor, and the global hotkeys with their switch and four boxes. Every change saves at once and applies live.

The file is `config.json` in the app config directory (`%APPDATA%\com.notchy.app\config.json`). The notification app list is `notification-apps.json` beside it. `config.rs` owns it: a missing or corrupt file gives the defaults, and saves are written through a temp file and renamed. A leading UTF-8 byte order mark (which Windows PowerShell writes) is skipped on load, so a hand-edited file keeps its settings. Launch at login is the `Notchy` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.

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

- Pill sizes are defined in both `state.rs` (hit-testing) and `Island.svelte` (rendering). Keep them in sync. The window is 720 x 240 (`tauri.conf.json` and `window.rs`), which holds the largest pill, the lyrics layout: 620 x 224 at rest, and up to 700 x 240 for a track with long lines (`lib/lyricsFit.ts`, bounds in `state.rs`). DESIGN.md has the full table.
- Album art is read once per track; an app that publishes its thumbnail late may show the placeholder.
- Only the primary monitor is supported.
- The volume and brightness HUD follows the default output and the built-in panel. External monitors show no brightness HUD, and the native Windows OSD still appears alongside it.
- One timer runs at a time, and its state is not kept across restarts. The stopwatch keeps 20 laps; later laps are ignored.
- Calendar reminders are unit-tested with fixed times, and the pill and page were checked in a browser harness. A real event firing in the Tauri window has not been checked.
- The lyrics layout was checked in a browser harness only, at rest and with a long line, not in the Tauri window. A lyric line too long for 700 x 240 even at 70% text is clipped at the notch's edge, so it is never drawn outside the notch.
- The media progress bar's seek, the snooze strip, and the form's length and reminder fields were not checked in a browser harness or in the Tauri window. Whether a given player accepts a seek is up to that player: a player that refuses it leaves the bar where it was.
- Global hotkeys were not tested against a real keyboard in the Tauri window. Whether a combination registers depends on what other apps hold, and it is reported in Settings rather than checked here.
- The island's month grid and its Add note button were checked in the browser harness, not in the Tauri window. Real pointer hits through the notch and the Settings window opening on a click are not verified.
- Battery alerts read the level every 2 s, so a change can show up that late. A PC with no battery never shows a pill. The pills were checked in a browser harness that drives the same events. A real plug or unplug has not been tested on the Tauri window.
- Notifications need Windows' "Let apps access your notifications" switch on. They are read by polling every 1.5 s, so a toast can show up to about 1.5 s late. There is no click action and no icon, and Focus assist is not checked. The notification pill also takes the cursor like the HUD does, so the notch does not click through there.
