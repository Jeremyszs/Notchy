# Notchy design system

Read this before you touch anything that draws, animates or sizes the notch: `src/Island.svelte`, the
pages (`TimerPage.svelte`, `ControlsPage.svelte`, `NetworkList.svelte`), `RollingClock.svelte`,
`IdleClock.svelte`, `lib/shape.ts`, `lib/timer.ts`, or the sizes in `src-tauri/src/state.rs`. Every
number below comes from the code. If you change a number, change it here too.

## 1. Design principles

- **Pitch black.** The notch is `#000`. Every shape fills with `#000`, and the only colour comes from
  content: album art, a timer accent, a HUD bar. Text is white at a fixed set of opacities (section 4).
- **Apple reference.** The island reads as a physical notch that grows. Shapes spring open from the top
  centre, the outline flares out of the top edge with concave ears, and the bottom corners stay convex.
  Nothing has a border or a shadow except the alert glow.
- **Only `transform` and `opacity` animate.** Layout is never animated. Widths and heights are set
  once per state, and each shape scales from its previous size. Every existing transition and keyframe
  follows this. Colour changes are instant, or they change `opacity` on a copy of the colour. Do not add
  `background-color`, `width`, `height`, `top` or `margin` transitions.
- **Restraint.** One accent per feature. Gradients only for the accent pairs, the HUD bars, the slider
  fill (a hard stop) and the lyrics tint. No decorative motion while a state is still (the equaliser and the timer ring move only
  because their data moves).
- **Reduced motion keeps meaning, not movement.** Motion is cut to a fade or a jump. Information still
  changes (see section 9).

## 2. Size table

Body sizes are logical px. The ears (outside the body) do not count for hit-testing. Each pill's
outline width is `w + 2 * ear`. The window is 720 x 240 (`tauri.conf.json` and `window.rs`), and the
largest pill is the lyrics layout.

| State (`IslandState`) | Body w x h | Ear | Bottom radius | Defined in |
| --- | --- | --- | --- | --- |
| `idle` | 120 x 30 | 10 | 15 (h/2) | `state.rs` `pill_size`, `Island.svelte` `SIZES` |
| `active` (media) | 200 x 30 | 10 | 15 | same |
| `activity` (timer) | 220 x 30 | 10 | 15 | same |
| `hud` | 260 x 30 | 10 | 15 | same |
| `alert` (timer finished) | 320 x 64 | 10 | 24 | same |
| `expanded` (panel) | 380 x 192 | 10 | 24 | same |
| `lyrics` (wide panel) | 620 x 224 | 10 | 24 | same |
| `tucked` | 64 x 5 | 2.5 | 2.5 | same |
| `hidden` | 0 x 0 | none | none | `state.rs` |

Rules:

- `state.rs` `pill_size` (hit-testing and click-through) and `Island.svelte` `SIZES` (rendering) must
  match. The Rust tests `expanded_pill_fits_the_window`, `lyrics_pill_fits_the_window`,
  `activity_and_alert_fit_the_window` and `hud_pill_is_slim_and_fits_the_window` check the window fit.
- The bottom radius is `min(h / 2, 24)`, from `Island.svelte` `islandPath(w, h, ear, radius, bleed)`.
- The outline bleeds 16 px above the window top (`BLEED`), and the window clips at y=0.

## 3. Layout system

### 3.1 Spacing scale

Use these values. Other values appear only where a control's own size requires them.

| Token | px | Used for |
| --- | --- | --- |
| hairline | 2 | segmented-control padding, the gap between tabs, track radius |
| tight | 4 | row gaps in the timer, tile label gap, pager dot gap |
| small | 6 | timer row gap, network row inner gap, the word and its dots |
| standard | 8 | panel page gap, pill gaps, slider stack gap, the gap above the pager |
| medium | 12 | controls grid gap, transport gap in media, activity side inset |
| safe top | 14 | top of every expanded or lyrics panel (`.panel`, `.wide`) |
| side | 16 | side inset of the expanded and lyrics panels |
| pager | 16 + 8 | pager row height, then the space under it |

### 3.2 The expanded panel (380 x 192)

```
y   0 .............................  top edge of the window (the ears flare here)
    14 px  safe top (.panel padding-top)
    146 px page box (.pages): the active page's content is centred here
           (Timer: a 24 px docked selector, a 6 px gap, then a 116 px lower area)
    8 px   gap (.pages margin-bottom)
    16 px  pager row (.pager), dots 5 px, centred in the row
    8 px   under the pager (.pager margin-bottom)
y 192 .............................  bottom edge
```

- **Side inset** is 16 px on both sides (`.panel` padding 0 16). The content box is 348 px wide.
- **Vertical centring rule.** A page's content block is centred in its 146 px box, so the gap above it
  equals the gap below it, within 1 px. Pages use `justify-content: center` with an explicit gap. Do
  not use `space-between` on a page root: it pins the first and last rows to the edges.
- **Docked controls.** The timer's mode selector is docked. It is the first row of the page box, 24 px
  tall, at y 14 in the window, and it is the same box in every mode and state. Only the lower area
  (`.lower`, the 116 px below the selector and its 6 px gap) adapts. Each mode's content is an absolute
  body that fills the lower area and centres its rows in it, so the gap above equals the gap below.
  A mode change is a 150 ms opacity crossfade between two bodies that overlap in the same space.
- **Lists are the exception.** A list view (Wi-Fi, Bluetooth) puts its header on the top edge of the
  page box and its footer on the bottom edge, and the scrolling list takes what is left. An empty
  list centres its message inside the list body (`margin: auto`).
- **Content never touches the pager.** The 8 px gap is the minimum between a page's last row and the
  pager row.

Measured in the shipped build (gap above / gap below the content block, px):

| Page | Above | Below | Content height |
| --- | --- | --- | --- |
| Media (art, title, lyric line, transport) | 4 | 4 | 138 |
| Controls (2 sliders, 6 tiles) | 5 | 5 | 136 |
| Timer, countdown (idle, running, paused, done) | 5 | 5 | 106 |
| Timer, stopwatch (idle, running, paused, 0 laps) | 20 | 20 | 76 |
| Timer, stopwatch with 2 laps | 1 | 1 | 114 |
| Timer, stopwatch with 7 laps (list scrolls) | 0 | 0 | 116 |
| Timer, pomodoro (focus, break, paused, done) | 9 | 9 | 98 |
| Wi-Fi and Bluetooth sub-views | header on top edge, footer on bottom edge | | 146 |

The timer rows in this table are measured against the lower area (116 px, top at y 30), below the
docked selector. The selector itself is measured at y 14, 24 px tall, x 94.8 from the panel's left
edge and 210.41 px wide, in every mode and state. Before the docking, the selector moved between
y 15 and y 34 (19 px) as the content changed height, because the whole page was centred as one block.

The stopwatch with 2 laps fills the lower area with 1 px either side. The laps list is only as tall as
its laps, up to the room left. With more laps the list takes the rest of the area (34 px here) and
scrolls, so the box never overflows.

### 3.3 The lyrics layout (620 x 224)

```
y   0 .............................
    14 px  safe top (.wide padding-top)
    178 px body (.wide-body): left column and lyric stack, each centred
    8 px   gap
    16 px  pager row
    8 px   under the pager
y 224 .............................
```

- Side inset is 16 px (`.wide` padding 0 16). The left column is 250 px wide, and the stack takes the
  rest of the 588 px body.
- The left column (art 88, gap 14, transport 68 = 170 px) is centred in the 178 px body: 4 px above and
  below. The lyric stack (132 px) is centred: 23 px above and below.

### 3.4 Compact pills (30 px tall, 64 for alert)

- Every pill centres its content vertically with `align-items: center` in the full body height.
  Measured gaps above and below: active 5/5, activity 7/7, HUD 6/6, alert 16/16, idle clock 9/9.
- Side insets are equal left and right for each pill: active 10, activity 12, HUD 14, alert 12.
- **The middle is never empty.** See section 8, rule 1.

### 3.5 Measuring a layout

The rule is measured, not eyeballed. In the browser, for the visible shape:

```js
const box = (el) => el.getBoundingClientRect();
const area = box(document.querySelector('.shape.on .pages'));   // the page box
const kids = [...document.querySelectorAll('.shape.on .page.on > *')]; // the content
const top = Math.min(...kids.map(k => box(k).top)) - area.top;
const bottom = area.bottom - Math.max(...kids.map(k => box(k).bottom));
// Centred when |top - bottom| <= 1
```

Use a union of child boxes, not the page's own box (a page root with `height: 100%` always fills its box).

## 4. Typography

- **Family.** `--font` is set on the root from `fontStack(config.font)` (`src/lib/font.ts`). The default
  is Inter. The choices are SF Pro (only when installed, otherwise Inter), Inter, Manrope, DM Sans,
  Space Grotesk, Playfair Display and System (Segoe UI Variable). The fonts are bundled as latin
  subsets (`src/fonts.css`). The notch never resizes when the font changes, so pick a font whose
  numbers fit the fixed widths.
- **Base.** `.content` is `500 13px/1.2 var(--font)`, white.
- **Sizes and weights in use.**

| Size / line-height | Weight | Where |
| --- | --- | --- |
| 36 / 40 | 600 | Timer and pomodoro digits (`.digits`) |
| 27 / 32 | 600 | Lyrics current line (`.stack .cur`, two lines max) |
| 21 / 26 | 400 | Lyrics context lines (`.stack .ctx`, 40% white) |
| 19 / 23 | 600 | Lyrics layout title (`.heading`, two lines max) |
| 16 | 400 | Timer stepper symbols |
| 15 / 18 | 400 | Lyrics layout artist |
| 14 / 17 | 600 | Panel title (`.title`), alert title |
| 13 / 16 | 500 | Panel lyric line (`.lyric .line`) |
| 13 | 600 | Activity digits (`.clock-slot`), tabular |
| 12 / 16 | 500 | Active pill title (`.active-title`) |
| 12 | 600 | Pills and chips, HUD value, stepper value, idle clock, section heading |
| 12 | 400 | Panel artist (60% white), network names |
| 11 / 14 | 400 | Notices, alert sub-line, lyric source line |
| 11 / 16 | 500 | Activity word ("Focus", "Break") |
| 11 / 16 | 400 | Timer laps, pomodoro cycle line |
| 11 | 600 | Timer mode tabs |
| 10 / 12 | 400 | Quick-control tile names, network state and links |

- **Tabular numbers.** Every number that changes while shown uses `font-variant-numeric: tabular-nums`
  (digits, activity clock, HUD value, idle clock, stepper value). `RollingClock` lays each digit in a
  fixed column, so the width of a clock does not move.
- **Truncation.** A single line truncates with `overflow: hidden; white-space: nowrap; text-overflow:
  ellipsis`. The active title fades instead (a 16 px mask on the right). Two-line text uses a two-line
  clamp (`-webkit-line-clamp: 2`).
- **Colours of text.** White at these opacities only: 100% (primary), 90% (line, title), 85% (glyphs),
  60% (artist, activity word, alert sub-line), 55% (idle clock, network state, inactive chip),
  50% (lyric source), 40% (lyric context), 35% (disabled), 16 to 20% (tracks, fills).

## 5. Colour

- **Shape fill:** `#000`. Ring cores are `#000` too, so only the ring band shows.
- **Per-feature accents** (from `palette()` in `src/lib/timer.ts`, and the HUD and media code):

| Feature | From | To | Notes |
| --- | --- | --- | --- |
| Timer (countdown) | `#FF9F0A` | `#FFD60A` | Apple orange to yellow |
| Focus (pomodoro) | `#FF453A` | `#FF7A59` | tomato |
| Short break | `#30D158` | `#7CF2A1` | mint |
| Long break | `#64D2FF` | `#64D2FF` | teal blue, flat |
| Stopwatch | `#FFFFFF` | `#8E8E93` | white to grey |
| Alert | the finished timer's palette | | ring, glow and the strong button |
| Media (active pill) | white bars and title | | no accent: the artwork is the colour |
| Media (lyrics tint) | hue from the album art, saturation 45 to 75%, lightness 28 to 40% (`src/lib/tint.ts`) | | fades to `#000` by 85% of the height |
| Volume (HUD) | `#7DD3FC` | `#FFFFFF` | ice |
| Brightness (HUD and icon) | `#FFD166` | `#FFF3C4` | amber |
| Mute slash | `#FF6B6B` | | HUD and quick-control icons |

- **Accent use.** An accent fills a ring, a strong pill, a filled pomodoro session, or the glow. It is
  never used for body text. A filled session dot is the accent at 100%, an empty one is the accent at
  25% opacity (`opacity`, not a colour transition).
- **Toggles.** Off is a 12% white disc with white glyph. On is a white disc with a black glyph.

## 6. Motion

Times and curves used in code. Keep them unless you have measured a reason.

| Motion | Duration | Curve | Property |
| --- | --- | --- | --- |
| Island slides (show or hide) | 450 ms | `cubic-bezier(0.22, 1, 0.36, 1)` | transform (`.island`), opacity 180 ms ease |
| Shape width (springs open) | 450 ms | `cubic-bezier(0.34, 1.3, 0.64, 1)`, peak about 1.03 | transform scaleX (`.shape`) |
| Shape height (must not overshoot) | 450 ms | `cubic-bezier(0.22, 1, 0.36, 1)` | transform scaleY (`.frame`) |
| Content leaves | 120 ms | ease | opacity |
| Content enters | 200 ms, after 120 ms | ease | opacity |
| Page change | 220 ms | `cubic-bezier(0.22, 1, 0.36, 1)` | transform (16 px slide) and opacity 220 ms ease |
| Timer mode crossfade (content below the selector) | 150 ms | linear (Svelte `fade`) | opacity only; the selector does not move |
| Panel sub-view (Wi-Fi, Bluetooth) | 220 ms, fly 24 px | cubic-out | transform; leaves with a 160 ms fade |
| Controls grid while a sub-view is open | 160 ms | ease | opacity |
| Lyric line change | 180 ms | Svelte fly, rises 4 px | transform and opacity |
| Lyrics stack change | 220 ms, rises 10 px; old block fades 180 ms | | transform and opacity |
| Lyrics tint crossfade | 400 ms | | opacity |
| HUD bar | 120 ms | ease-out | transform scaleX |
| HUD icon | 160 ms in, 110 ms out | | opacity |
| Timer digit roll | 220 ms | `cubic-bezier(0.2, 0.8, 0.2, 1)`, 150 ms crossfade under reduced motion | transform and opacity |
| Last-ten-seconds beat | 300 ms | ease-out, scale 1 to 1.06 to 1 | transform |
| Finished ring ripple | 900 ms | `cubic-bezier(0.2, 0.8, 0.2, 1)`, scale 1 to 2.6 | transform and opacity |
| Alert glow | 2400 ms | ease-out, keyframes 0 to 0.6 to 0.16 | opacity |
| Equaliser bars (fallback) | 900 ms | ease-in-out, alternate, infinite | transform |
| Equaliser bars (live) | 70 ms | ease-out per 30 Hz frame | transform |
| Timer ring sweep | 1 s | linear | transform on each half disc |
| Paused digits | 320 ms | ease | opacity |
| Press feedback on tiles | 120 ms | ease | transform scale 0.94, opacity 0.7 |
| Armed lock tile | | | transform scale 1.1, outline 2 px |
| Spinner and pulse | 800 ms linear; 1200 ms ease-in-out | | transform, opacity |

Rules:

- **No layout properties in transitions.** Height changes in content use a fixed height and
  `transform` or `opacity`. The timer's `.digits` is a fixed 40 px so a mode change never moves the rows
  below.
- **Scroll is not animated.** Lists scroll natively, and the wheel is kept inside the list until it
  reaches an end (`keepWheel` in `NetworkList.svelte`).
- **Per-second updates are the only steady motion.** The timer ring and the digits update once a
  second, and the stopwatch reading updates once per frame while it runs. The idle clock updates once a
  minute.

## 7. Components

### Pill ears and outline
`islandPath(w, h, ear, radius, bleed)` draws the body with the top flare. The flare is a concave quarter
circle of radius `ear` (10, or 2.5 for the tucked sliver), so the notch meets the screen edge. The
outline is an absolutely positioned SVG with `top: -16px`, so it bleeds above the window top.

### Timer ring
Two half discs, `.half.right` and `.half.left`, each clipped to its half of a 16 px (activity) or 22 px
(alert) circle. Each disc rotates inside its half. `ringRotations(fraction)` gives the two angles. A
2 px band shows, because the core is inset 2 px and filled with `#000`. The track is white at 16%.

### Rolling clock
`RollingClock.svelte` lays each character in a fixed column (`.column`, inline-grid, centred). A digit
that changes rolls out upward while its replacement rises from below (`roll` action, 220 ms). A digit
that does not change is not re-keyed, so it stays still. Separators (`:`) are 0.3 em wide.

### Idle clock
`IdleClock.svelte`, inside the idle pill when `idleClock` is on. Text is 12 px, weight 600, 55% white,
`line-height: 1`, tabular. It reads `minuteClock` (`src/stores/clock.ts`), which arms one timeout per
minute for the next boundary. Nothing polls. The store runs only while the component is mounted, so
turning the setting off removes the timer.

### Activity pill
Three columns in a grid: `52px | 1fr | 52px`. Left is the ring (or the stopwatch glyph, 16 px). Middle
is the word (11 px, 60% white, ellipsis) and, in pomodoro, one 4 px dot per focus session (accent at
100% when done, 25% when not). Right is the clock (13 px, 600, tabular). The word is one of Timer,
Stopwatch, Focus, Break, Long break, Paused or Done (`activityLabel` in `src/lib/timer.ts`). The middle
column is 92 px, which holds "Long break" (59 px), a 6 px gap and four dots (25 px).

### Rows (network lists)
28 px tall, 8 px radius, `0 6px` padding, 12 px text, hover fill at 8% white. Signal bars are 14 x 11
px, four bars, unlit bars at 25% opacity. A lock is 10 px at 55%. A check is 12 px. Icons are 16 px.

### Tiles (quick controls)
A tile is 48 px wide: a 44 px round (`.round`), a 4 px gap and a 12 px label line (10 px text). The
icon is 20 px. The round is 12% white when off and white when on. The Wi-Fi and Bluetooth tiles have a
20 px chevron hit area at the top right, drawn as a black disc with a 1 px rim. Six tiles fill 348 px
with 12 px between them. The page holds at most two sliders and seven tiles.

### Sliders
28 px tall, with an 8 px track that has a 4 px radius. The fill is a hard gradient stop driven by `--p`
(`calc(8px + (100% - 16px) * v)`), so no property animates. The thumb is transparent and 16 px. The
glyph is 18 px.

### Pills and chips (timer and alerts)
- Timer buttons: 28 px tall, 14 px radius, 14 px side padding, 12 px 600 text. The strong variant is a
  gradient from `--c` to `--c2` with black text. The quiet variant is text only.
- Alert buttons: 26 px tall, 13 px radius, 11 px side padding.
- Presets (timer): 24 px tall, at least 44 px wide, 12 px radius.
- Stepper: 28 px round buttons with a 6 px gap, value 56 px wide, tabular.
- Mode tabs: a 24 px tall track with 2 px padding, each button 20 px tall with 10 px side padding.

### Pager
Row 16 px tall, 4 px gaps. Each dot is a 16 px hit area with a 5 px white dot at 35% opacity (100%
when current). The row is centred, with 8 px under it.

### Lyrics transport
Three round buttons: skip 60 px, play 68 px, glyphs 36 px and 40 px, 12 px gap, centred under the art.

## 8. Mistakes found in the audit, and the rule that prevents each

| # | Mistake (measured) | Rule |
| --- | --- | --- |
| 1 | Compact activity pill had an empty middle between the ring and the time. | Never leave an empty middle in a compact pill. Put the word and any session dots there. |
| 2 | Timer page pinned its rows to the top with 20 to 48 px empty below (`space-between`). | Centre page content in the page box. Never use `space-between` on a page root. |
| 3 | Media and controls pages were flush to the top and bottom of the page box, touching the pager. | Keep at least 8 px between content and the pager, and check it with a measurement. |
| 4 | The lyrics body touched the pager (0 px gap), and the left column sat 6 px from each edge. | Give the body an 8 px bottom margin, and size the body so columns have room on both sides. |
| 5 | The stopwatch with laps filled its page box exactly, so the laps list touched the pager (0 px gap). | Leave room under a page's last row. Size lists to whole rows, and check the gap in the measurement. |
| 6 | Empty states (Wi-Fi, Bluetooth) sat 12 px under the header, not centred in the list. | Centre an empty message in its list body. |
| 7 | Expanded and lyrics panels had different side insets (16 and 14). | Use the side inset of 16 px everywhere a panel has content. |
| 8 | Timer dots changed colour with `background-color`. | Animate only `opacity` on a copy of the accent colour. |
| 9 | An alert measured mid-ripple read 25 px taller than its content (the ring was scaled). | Measure alerts at least 1.8 s after they open. The ripple is a transform, so it must have finished. |
| 10 | The idle pill had no content, so its middle was blank. | Keep the idle pill minimal and make any content a setting that defaults on (section 10). |
| 11 | The timer's mode tabs moved up and down (15 to 34 px, 19 px) as each mode's content changed height, because the whole timer page was centred as one block. | Controls that switch modes are docked; only the content below adapts. Give that content the remaining height and centre it there (section 3.2). |

## 9. Accessibility and reduced motion

- Reduced motion is read in two places: the CSS media query `(prefers-reduced-motion: reduce)`, and a
  module-level `matchMedia` check in `Island.svelte`, `ControlsPage.svelte` and `RollingClock.svelte`.
  The JavaScript check is read once when the module loads, so a change while the app runs needs a
  restart. Keep both in step when you add a motion.
- Under reduced motion: shapes and pages change by fade only (150 ms) with no transform, the equaliser
  bars are static, the HUD bar and the ring jump instead of gliding, the timer digits crossfade over
  150 ms, the ripple is hidden, the alert glow sits at 35%, the lyric line and stack do not rise, the
  spinner pulses instead of turning, and the tile press does not scale. The timer's mode change is a
  150 ms opacity crossfade in both settings, because it is a fade and carries no motion.
- Focus rings: a 2 px outline at 70% white in most places. The pager dots use 1 px at 60%. Never remove
  an outline without a replacement.
- Text contrast: the lowest text opacity in use is 40% (lyric context lines). Disabled controls drop to
  35% to 40% opacity. Do not go below 40% for text that carries information, and check any new opacity
  against black.
- The panels, the alert and the lyrics layout are `role="group"` with a label, and the alert text is a
  `role="status"` region. The activity and HUD pills are `aria-hidden`, because the panel and the alert
  already say the same thing. Pages that are not shown are `inert`, so they take no focus or taps.

## 10. Settings that change the notch

Each setting lives in `config.json` (`src-tauri/src/config.rs`), mirrors in `src/stores/config.ts`, and
has a row in `src/settings/Settings.svelte`.

| Setting | Default | Effect |
| --- | --- | --- |
| `pages` | media, controls, timer | Panel pages and their order. One page hides the pager. |
| `controls` | eight ids | Quick controls and their order (sliders first, at most seven tiles). |
| `lyrics` | on | Synced lyrics layout. |
| `confirmLock` | on | First tap arms Lock for 3 s. |
| `hud` | on | Volume and brightness HUD. |
| `timerSound` | on | Beep when a countdown ends. |
| `idleClock` | on | Shows HH:MM, dim, in the idle pill. |
| `focusMinutes`, `shortBreakMinutes`, `longBreakMinutes` | 25, 5, 15 | Pomodoro lengths. |

Decision on the idle clock: the idle pill is the only moment the notch is not doing something, so
it is the one place a glance at the time costs nothing. A 12 px, 55% white `HH:MM` fits inside the
existing 120 px pill with 42 px either side. The outline and silhouette do not change, so the notch
still reads as a physical notch. It is on by default, and Settings turns it off for anyone who wants
the blank notch.

## 11. Pre-merge checklist for UI changes

1. `npm run check`, `npm test`, `npm run build` all pass.
2. If Rust changed: `cargo test` and `cargo clippy --all-targets -- -D warnings` in `src-tauri`, with
   `CARGO_TARGET_DIR` set to a temporary directory.
3. If a size changed: `state.rs` `pill_size`, `Island.svelte` `SIZES`, this file (section 2), and the
   README's known-limits note all agree. The window still holds the largest pill.
4. Every page and state you touched is measured (section 3.5). Centred blocks have top and bottom gaps
   within 1 px. Pills have equal side insets. Nothing overflows its box. Truncated text has
   `scrollWidth <= clientWidth` plus the ellipsis, or the fade.
5. Screenshots, before and after, for the states you touched: idle (with and without the clock), active,
   each activity mode (timer, stopwatch, pomodoro focus and break, paused, done), HUD, alert, expanded
   (media, controls, Wi-Fi and Bluetooth sub-views, timer in each mode), lyrics, tucked.
6. Motion uses only `transform` and `opacity`. Reduced motion still fades and still shows the change.
7. No new text colour outside the set in section 4.

## 12. How to test UI

Do not use port 1420. That is the user's `tauri dev` server. Use Vite on a free port such as 1430:

```
npx vite --port 1430 --strictPort --host 127.0.0.1
```

Run it in the background, and stop it at the end. Do not commit a test harness.

The harness is a temporary HTML page at the project root. It mocks the Tauri IPC, which the stores use,
before the app module loads:

- `window.__TAURI_INTERNALS__.transformCallback` stores each listener by id, and `invoke` answers
  `plugin:event|listen` by recording `event -> handler id`. An emit calls the stored callback with
  `{ event, id, payload }`.
- `invoke` answers each command the stores call (`config_get`, `media_snapshot`, `lyrics_snapshot`,
  `timer_snapshot`, `controls_list`, `wifi_networks`, `bluetooth_devices`, and so on).
  **Gotcha:** a snapshot command that returns `null` makes a store's listener set `null`, and the Svelte
  component throws and stops updating. `lyrics_snapshot` must return an object (for example
  `{ key: "", status: "none", lines: [], plain: null }`).
- Set state by emitting `island://state`, `timer://update`, `media://update`, `lyrics://update`,
  `hud://update` and `timer://finished`. Wait about 900 ms for the springs, and 1.8 s after an alert.
- Click the pager dots (`.shape.on .pager .dot`) to change page, and the Wi-Fi or Bluetooth chevron to
  open a sub-view.

Measure with `getBoundingClientRect` (section 3.5). Take screenshots through the Browser pane. Its
viewport can be narrower than 380 px, so set `document.documentElement.style.zoom = 0.75` while you
shoot, and reset it to `""` afterwards.

Stop the server and delete the harness files before you finish. `git status` should show only the real
changes.

## 13. Known limits

- The real Tauri window is not part of the browser harness. Click-through, the window's placement and
  the Settings window's appearance are checked in `tauri dev`.
- Timer digits are measured by line box, not by ink. A font with a tall or short cap height can look
  a pixel off-centre. Adjust `line-height` on the element, not the padding, if that happens.
