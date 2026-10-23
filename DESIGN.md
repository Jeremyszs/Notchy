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
largest pill is the lyrics layout: 620 x 224 at rest, and up to 700 x 240 for a track with long lines
(section 3.3). 700 plus two 10 px ears is 720, so the window holds it.

| State (`IslandState`) | Body w x h | Ear | Bottom radius | Defined in |
| --- | --- | --- | --- | --- |
| `idle` | 120 x 30 | 10 | 15 (h/2) | `state.rs` `pill_size`, `Island.svelte` `SIZES` |
| `active` (media) | 200 x 30 | 10 | 15 | same |
| `activity` (timer) | 220 x 30 | 10 | 15 | same |
| `hud` | 260 x 30 | 10 | 15 | same |
| `notification` (another app's toast) | 260 x 30 | 10 | 15 | same |
| `battery` (charger, full, low battery) | 260 x 30 | 10 | 15 | same |
| `calendar` (event reminder) | 260 x 30 | 10 | 15 | same |
| `alert` (timer finished) | 320 x 64 | 10 | 24 | same |
| `expanded` (panel) | 380 x 192 | 10 | 24 | same |
| `lyrics` (wide panel) | 620 x 224 at rest, up to 700 x 240 | 10 | 24 | `state.rs` `pill_size` (between `LYRICS_MIN` and `LYRICS_MAX`), `Island.svelte` `SIZES` and `fitLyrics`, the size from `lib/lyricsFit.ts` |
| `tucked` | 64 x 5 | 2.5 | 2.5 | same |
| `hidden` | 0 x 0 | none | none | `state.rs` |

Rules:

- `state.rs` `pill_size` (hit-testing and click-through) and `Island.svelte` `SIZES` (rendering) must
  match. The Rust tests `expanded_pill_fits_the_window`, `lyrics_pill_fits_the_window`,
  `activity_and_alert_fit_the_window` and `hud_pill_is_slim_and_fits_the_window` check the window fit.
- The lyrics layout is the one size that changes with its content. `lib/lyricsFit.ts` picks the size for
  each track, from all its lines, and `Island.svelte` reports it to Rust with `island_set_lyrics_size`, so
  hit-testing follows the drawn body. `state.rs` holds it between 620 x 224 and 700 x 240
  (`lyrics_size_is_held_between_rest_and_the_window_limit`).
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
| Calendar (month grid and notes) | 3 | 3 | 140 |

The timer rows in this table are measured against the lower area (116 px, top at y 30), below the
docked selector. The selector itself is measured at y 14, 24 px tall, x 94.8 from the panel's left
edge and 210.41 px wide, in every mode and state. Before the docking, the selector moved between
y 15 and y 34 (19 px) as the content changed height, because the whole page was centred as one block.

The stopwatch with 2 laps fills the lower area with 1 px either side. The laps list is only as tall as
its laps, up to the room left. With more laps the list takes the rest of the area (34 px here) and
scrolls, so the box never overflows.

### 3.3 The lyrics layout (620 x 224 at rest, up to 700 x 240)

```
y   0 .............................
    14 px  safe top (.wide padding-top)
    h - 46 px body (.wide-body): left column and lyric stack, each centred
           (178 px at 224, 184 at 230, 194 at 240)
    8 px   gap
    16 px  pager row
    8 px   under the pager
y h .............................
```

- Side inset is 16 px (`.wide` padding 0 16). The left column is 250 px wide. The lyric stack takes the
  rest after the 20 px gap, so its text width is `w - 302` px: 318 at 620 and 398 at 700.
- The left column is the title and artist block (`.now-wide .meta`, 88 px: a two-line title of 28 px
  lines, an 8 px gap and the 24 px artist line), level with the 88 px art. Then a 10 px gap, the progress
  row (14 px), a 10 px gap and the transport (52 px): 174 px in all, centred in the 178 px body, so 2 px
  above and below at rest. The title is 24 / 28 at weight 600, two lines at most, and the artist is 17 / 24.
  The block spans the art's 88 px with a one-line title as well.
- The lyric stack (`.stack`) has the current line at 27 / 32 weight 600, in a box at least 64 px tall
  (two rows), and each context line at 21 / 26, 40% white, in a box at least 26 px tall, with 8 px
  gaps. Short lines give 132 px, centred: 23 px above and below.
- Lines wrap whole. `overflow-wrap: anywhere` breaks a word longer than the column. Nothing is clipped,
  and no ellipsis is drawn. `--ls` on `.content` scales the text, and it is 1 unless the fit needs less.

**Fit (`src/lib/lyricsFit.ts`).** The size is worked out for the track from all its lines, so it stays
still while the lines change. The steps are in this order, and each one is the smallest that works:

1. Widen from 620 to 700 at 224 high, until every line shows whole as the current line or as a neighbour.
2. At 700 wide, grow the height to the body the lines need (up to 240).
3. Only then scale the text: 0.95, 0.9 and so on down to a floor of 0.7.

A line's rows are counted with the measured width of its words at the stack's text width. Words wrap at
spaces, and a longer word breaks by character. A current line needs `(max(2, rows) * 32 + (prev + next)
* 26) * scale + 16` px of body, where each neighbour counts at least one row, and the pill is that plus
46. The size is the largest of these over the track's lines. In the browser harness a 97-character line
at 700 wide wrapped onto three rows, and the pill was 700 x 230, with the 96 px current box holding all
three rows (`.cur` and its text the same height).

### 3.4 Compact pills (30 px tall, 64 for alert)

- Every pill centres its content vertically with `align-items: center` in the full body height.
  Measured gaps above and below: active 5/5, activity 7/7, HUD 6/6, alert 16/16, idle clock 9/9,
  notification 7/7.
- Side insets are equal left and right for each pill: active 10, activity 12, HUD 14, alert 12.
- **Notification pill.** Three columns, `16px | 8px gap | 1fr | 8px gap | 16px`, with 12 px side
  padding. The badge sits in the first column and the text in the middle one, and the last column is
  an empty 16 px that mirrors the badge, so the text centre is the pill centre (measured: both at the
  window centre). The text is one line, 12 px, with the title at 600 and 90% white and the body at
  400 and 60% white. A long line fades out with an ellipsis, and a title-only notice has no body span.
- **Battery pill.** The glyph and its words are one flex row, centred as a group (`justify-content:
  center`, 8 px gap, 12 px side padding). The group's centre is the pill's centre, so the gaps on
  each side are equal whatever the words are. Measured: group centre within 0.01 px of the pill
  centre, left and right gaps equal (66.1 px for "On battery 82%"), glyph and text centred
  vertically (0 px). The text never overflows.
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
| 24 / 28 | 600 | Lyrics layout title (`.heading`, two lines max) |
| 16 | 400 | Timer stepper symbols |
| 17 / 24 | 400 | Lyrics layout artist |
| 14 / 17 | 600 | Panel title (`.title`), alert title |
| 13 / 16 | 500 | Panel lyric line (`.lyric .line`) |
| 13 | 600 | Activity digits (`.clock-slot`), tabular |
| 12 / 16 | 500 | Active pill title (`.active-title`) |
| 12 / 16 | 600 and 400 | Battery pill: the word (600, 90% white) and the level (400, 60% white, tabular) |
| 22 / 26 | 600 | Stats tile figure (`.value`, tabular, a fixed 26 px line box) |
| 12 | 600 | Pills and chips, HUD value, stepper value, idle clock, section heading |
| 12 | 400 | Panel artist (60% white), network names |
| 11 / 14 | 400 | Notices, alert sub-line, lyric source line |
| 11 / 16 | 500 | Activity word ("Focus", "Break") |
| 11 / 16 | 400 | Timer laps, pomodoro cycle line |
| 11 | 600 | Timer mode tabs |
| 10 / 12 | 400 | Quick-control tile names, network state and links |
| 11 / 16 | 500 and 600 | Calendar page day numbers (tabular, 90% white), and a note's time (600, 60% white) |
| 11 / 12 | 600 | Calendar page weekday initials, 55% white |
| 12 / 18 and 12 / 24 | 600 and 500 | Calendar page selected date and "Add note" (600), and a note's title (500, 90% white) |
| 12 / 16 | 600 and 400 | Calendar reminder pill: the title (600, 90% white) and "in N min" (400, 60% white, tabular) |
| 13 / 18 | 600 | Calendar page month title, between the two arrows |
| 13 / 16 | 400 | Calendar page "No notes", centred at 55% white |
| 15 / 20 | 600 | Settings month title (`.title`), 160 px box, centred between the arrows |
| 14 / 17 | 600 | Settings month day heading (`.day-title`), 90% white |
| 14 / 14 | 500, 600 when coloured or selected | Settings month day numbers (`.day`), tabular, in a 36 px circle |
| 11 / normal | 600 | Settings month weekday labels, 55% white |
| 13 / 16 | 600 | Settings month "Today" button |
| 15 / 20 | 400 | Settings note title, and 600 for "Add note" and Save |
| 13 / 18 | 400 | Settings note time and notes line (55% white); status line (red, #FF453A) |
| 13 / 16 | 400, and 600 when on | Settings note form: length and reminder chips (on: white chip with a black label) |
| 13 / 18 | 400 | Settings hotkey conflict line (red, #FF453A), under the action it belongs to |
| 11 / 14 | 400 | Media progress times, elapsed and length (tabular, 60% white) |
| 12 / 24 | 500 | Calendar page snooze title (90% white), one line with an ellipsis |
| 11 / 20 | 600 | Calendar page snooze buttons, "5 min" and "10 min" (accent text) |

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
| Notification (pill) | `#0A84FF` | `#64D2FF` | blue. The badge only: the text stays white |
| Battery, charger and full | `#30D158` | | solid glyph fill (no gradient). Green is the charger |
| Battery, on battery | `#FFFFFF` | | solid glyph fill, neutral |
| Battery low, 20% | `#FF9F0A` | | solid glyph fill, amber (Apple orange) |
| Battery low, 10% | `#FF453A` | | solid glyph fill, red (Apple red) |
| Volume (HUD) | `#7DD3FC` | `#FFFFFF` | ice |
| Brightness (HUD and icon) | `#FFD166` | `#FFF3C4` | amber |
| Mute slash | `#FF6B6B` | | HUD and quick-control icons |
| Stats CPU | `#0A84FF` | | sparkline stroke only. Text stays white |
| Stats memory | `#BF5AF2` | | sparkline stroke only |
| Stats network (down) | `#30D158` | | sparkline stroke only |
| Stats disk | `#FF9F0A` | | sparkline stroke only |
| Calendar reminder | `#FF6A3D` | | the calendar glyph only (a stroke). The text stays white |
| Settings month, day with notes | `#FF6A3D` at 22% fill | | the day number in `#FF6A3D` (4.8:1 on `#111`) |
| Settings month, selected day | `#FF6A3D` | | solid circle, black number (7.4:1) |
| Settings month, "Add note" and Save | `#FF6A3D` | | link text (18.9:1 on `#111`), Save is solid with white text |
| Island calendar page, day with notes | `#FF6A3D` at 22% fill | | the day number in `#FF6A3D` (about 5.7:1 on that fill, computed) |
| Island calendar page, selected day | `#FF6A3D` | | solid disc, black number (7.4:1) |
| Island calendar page, "Add note" | `#FF6A3D` | | link text (7.4:1 on `#000`) |
| Island calendar page, snooze buttons | `#FF6A3D` | | link text (7.4:1 on `#000`), with a 14% accent fill on hover |
| Media progress bar | `#FFFFFF` | | the fill, on a track at 16% white. No accent: the artwork is the colour |
| Settings form, chips on | `#FFFFFF` | | a toggle that is on: white with a black label, as the switches are |

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
| Battery glyph fill | 320 ms | `cubic-bezier(0.22, 1, 0.36, 1)` | transform scaleX, origin left |
| Calendar reminder text change | 200 ms in, 120 ms out | ease | opacity (keyed on the reminder's `seq`) |
| Battery text change | 200 ms in, 120 ms out | ease | opacity (keyed on the event) |
| HUD icon | 160 ms in, 110 ms out | | opacity |
| Notification text change | 200 ms in, 120 ms out (fade, and the outgoing text stays in the same cell) | ease | opacity |
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
| Settings month slide | 220 ms, the new month slides 16 px; the old one fades 120 ms | `cubic-bezier(0.22, 1, 0.36, 1)` | transform and opacity. Reduced motion: a 150 ms fade, no slide |
| Island calendar month slide | 220 ms, the new month slides 16 px; the old one fades 120 ms | cubic-out (Svelte `fly`) | transform and opacity. Reduced motion: a 150 ms fade, no slide |
| Media progress fill | 250 ms, linear, one step per tick (the clock ticks 4 times a second while it runs) | linear | transform scaleX. A drag has no transition, and reduced motion jumps |

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

### Battery pill
A 260 x 30 pill, shown as `IslandState::Battery` (`state.rs`). The glyph is an SVG on a 25 x 14
viewBox: a 1.5 px outline, a track at 16% white, the fill, a 2 px nub, and a bolt on the charger.
The fill is a `transform: scaleX(level)` on a rect with `transform-box: fill-box` and origin left, so
a change glides and nothing animates `width`. The bolt is white with a thin black keyline, so it reads
over the fill and over the track. Charging and full charge show the bolt. Low battery and unplugged do
not.

Precedence: a battery pill is a transient. It sits under the HUD and the alert, and over a
notification. A plug event is short and physical, so it is not held back behind a message. While it is
up, the notification keeps its own timer, so a message can expire unseen under it, as it does under the
HUD. Hover and fullscreen hide it as they hide the HUD. Only one pill is on screen. A newer event
replaces it and restarts its time. Charger and full pills hold 3 s, and low-battery warnings hold 5 s.

### Stats page
`StatsPage.svelte` fills the 146 px page box with a 2 x 2 grid (8 px gap, so each tile is 170 x 69).
Each tile is a 12 px radius at 8% white, with a 12 px label row (11 px, 60% white, and an optional
55% detail on the right), the 22 px figure with its unit (12 px, 600, 60% white), and a 14 px
sparkline of the last 30 readings, drawn with `vector-effect: non-scaling-stroke` at 1.5 px. The
figure's digits roll through `RollingClock`, and the unit stays put. The units are KB/s, MB/s and
GB/s (one decimal above KB/s), and a missing reading shows an en dash. A tile's content is centred
with 4.5 px above and below. The sampler runs only while the expanded panel is open on this page
(`watchStats` in `src/stores/stats.ts`). It reads at 1 s, and the first reading has no rates.

### Calendar reminder pill
A 260 x 30 pill, shown as `IslandState::Calendar` (`state.rs`). A 16 px calendar glyph in the calendar
accent (#FF6A3D, a 1.5 px stroke), then the event title and "in N min" or "now". The glyph, title and
time are one group, centred with `justify-content: center` and 8 px gaps, as the battery pill is. The
title shortens with an ellipsis, and the time never shrinks. Measured at 0.75 zoom: the group's centre
is within 0.01 px of the pill's centre, the left and right gaps are equal, and the glyph and text are
centred vertically (0 px). The pill takes no clicks.

Precedence: a reminder sits under the battery pill, the HUD and the timer alert, and over a notification,
so a toast from another app never hides it. It is held for 6 s (a lead-time reminder) or 8 s (the start,
or an all-day reminder). Reminders queue one at a time, the way notifications do, up to five waiting.

### Calendar page (island)
The Calendar page (`src/CalendarPage.svelte`) shows the month on the left and the selected day's notes
on the right, in the 146 px page box. One CSS grid holds both columns, so they share rows. The block is
140 px tall, which leaves 3 px above and 3 px below in the box. Its width is 348 px, the content box.

| Part | Size (px) |
| --- | --- |
| Month column | 182 wide: seven 26 px columns |
| Divider | 1 px wide, 140 tall, 12 px from the grid and 12 px from the notes |
| Notes column | 141 wide |
| Heading row (month title and arrows; the date and "Add note") | 18 tall |
| Gap, weekday row, gap | 4, 12, 4 |
| Day rows | six 17 px rows, 102 in all. Always six, so the page never changes height |
| Arrow buttons | 24 x 18 each, so the title centres between them |
| Day hit area | the whole 26 x 17 cell. The number is a 16 px disc inside it |
| Notes body | 102 tall, the height of the six day rows, with the content centred in it. With the snooze strip: 72 tall, then a 6 px gap and the 24 px strip |
| Note row | 24 tall. Up to four rows: the notes, or three notes and a "+N more" row. With the snooze strip, up to three rows |

Text: the month title 13 / 18 600 (100% white). Weekday initials 11 / 12 600 at 55%. Day numbers 11 / 16
500, tabular, at 90% (40% outside the month). The selected date 12 / 18 600 at 90%, and "Add note" 12 / 18
600 in the accent. A note's time 11 / 16 600, tabular, at 60%. Its title 12 / 24 500 at 90%, one line
with an ellipsis. "+N more" 12 / 24 at 55%. "No notes" 13 / 16 at 55%, centred.

Colour: a day with notes is the accent at 22% with accent text. The selected day is a solid accent disc
with a black number. Today is a 1 px inset ring at 55%, unless it is selected. A day can be both.

Measured in the browser harness (720 x 240 viewport, expanded panel, px at 1x):
- Page box 146 (y 14 to 160). The 140 px block is 3 above and 3 below.
- Title centre minus the centre of the arrow row: 0. Title centred between the arrows: 0.
- Each weekday label's centre minus the centre of the day under it: 0, in all seven columns.
- Divider gaps 12 / 12. The notes body's top and bottom match the day rows' top and bottom (0 / 0).
  "Add note" is flush with the notes column's right edge, and the date row is level with the arrows.
- Four note rows (six notes): 3 / 3. Rows measured 24 each. No element overflows.
- Days checked: no notes ("No notes" centred), one note (a single all-day row), three notes (rows with
  an ellipsis on a 38-character title) and six notes ("+3 more"). Months checked: October 2026 (six rows),
  February 2026 (the last two rows are March), and December 2026 to January 2027 (the year end, with a
  coloured 31 December).

Behaviour:
- A click selects a day, and the month follows it (a click on a day from the next month moves there).
  The arrows move one month and keep the day, clamped to the month's length. The wheel only turns pages.
- The grid refreshes when the event list changes (`calendar://update`), and on a month change. The
  refresh keeps the same nodes, so it does not blink. Opening the panel selects today.
- A month change is a 220 ms slide of 16 px with a fade, and the old month fades in 120 ms. Reduced
  motion is a 150 ms fade with no slide. Only `transform` and `opacity` animate.
- "Add note" calls `settings_open_calendar(date)`. It opens Settings on that day with the add form. A
  window that is already open gets the `calendar://add-note` event. A window built for it takes the day
  once when it loads (`calendar_take_add_date`). The island cannot take text, so Settings does the typing.

Clicks are hit-tested by the pill's size like the Controls tiles. The clicks were driven in the browser
harness, not in the Tauri window (see section 13).

**Snooze strip.** When a reminder can be snoozed (`recentReminder` in `stores/calendar.ts`, from
`calendar://recent`), the bottom of the notes column holds a strip. The notes body gives up 30 px, which is a 6 px
gap and a 24 px strip, so the body is 72 px tall and holds three note rows (`noteRows(count, 3)`). The strip
has the reminder's title (12 / 24 500 at 90%, one line with an ellipsis) and two 20 px accent buttons, "5 min"
and "10 min" (11 / 20 600 in `#FF6A3D`), with 4 px between them. The title takes what the buttons leave. A
failed snooze puts its error text in the title until a new reminder arrives. With no snooze the body is the
full 102 px again, so the grid and the page do not move when the strip appears. The strip has no motion.

### Calendar month (Settings)
`CalendarMonth.svelte` is the Calendar section of Settings. A month grid sits in a card above a panel for the selected day. The card's side padding is the group padding (16 px), so the grid lines up with the rows of the groups above and below.

Measured in the browser harness at a 560 px window (the Settings minimum). The card is 512 px wide, and the grid is inset 16 px on each side.

- **Header.** A 32 px bar in a `1fr auto 1fr` grid. The middle is two 32 px arrows around a 160 px title box, so the title is centred between them (measured within 0.01 px of the arrows' midpoint) and nothing moves when the month name changes width. "Today" sits at the right end.
- **Weekdays.** A 24 px row, 8 px under the bar. Labels start on the first day of the week for the PC's language (`Intl.Locale.getWeekInfo`, Monday when unknown): Sunday in en-US, Monday in en-GB.
- **Grid.** A 240 px frame, always six rows of 40 px, so the card is the same height in every month. Seven equal columns of `(card - 32) / 7`, which is 68.56 px at 512 px. A weekday's centre and the centre of the day under it agree within 0.01 px.
- **Day.** A 36 px round button centred in its 40 px cell. Days outside the month are 40% white. A day with notes is the accent at 22% with accent text. The selected day is solid, with a black number at weight 600: black on `#FF6A3D` is 7.4:1, where white would be 2.85:1, below the 4.5:1 for small text. Today has a 1.5 px inset ring at 55% white, unless it is selected.
- **Panel.** A 44 px heading with the full date. A note row is at least 56 px: the title (15 px), a time line with the repeat word (13 px, 55%), an optional notes line (13 px, 55%, one line), and Edit and Delete as 32 px buttons. Delete keeps its confirm step. An empty day shows "No notes" in a 56 px row, then the 44 px "Add note" row.
- **Form.** The rows of the Settings groups: 12 px by 16 px fields, 8% white hairlines. A timed event shows Length (a select: no length, 15 minutes to 2 hours, or Custom, which shows a minutes field beside it, from 1 to 1440) and Remind me (chips: Default, which is the Settings lead time, or any of 5 min, 10 min, 15 min, 30 min, 1 hour and 1 day, up to five at once; none picked means only at the start). All-day events show neither, and save with no length and no reminders. Adding shows the selected date as a label. Editing shows a date input, and a repeating note says that every occurrence changes. The start time is the next full hour.
- **Keyboard.** The grid has the grid role and a roving tab stop on the selected day. Arrow keys move the selection a day or a week, Home and End move to the ends of the week, and PageUp and PageDown move a month and keep the day (clamped to the month's length). Focus follows the selection. Each day's label is the full date and its count, such as "Friday 23 October, 2 notes".
- **Data.** The grid asks `calendar_between` for its six weeks. It asks again when the event list changes (`calendar://update`), and an answer that arrives late is dropped.

Measured in the harness: 0 notes (2 October), 1 note (6 October), 4 notes (23 October, one of them the daily standup), the 240 px frame in every month opened (June, July, September, October 2026, February 2026 and February 2028), and February 2028 with its 29th coloured by the leap-day series and the clamped monthly rent. The narrow browser pane (355 px) is not a Settings size: there the add form's repeat select cuts "Does not repeat" short.

### Pager
Row 16 px tall, 4 px gaps. Each dot is a 16 px hit area with a 5 px white dot at 35% opacity (100%
when current). The row is centred, with 8 px under it.

### Media progress line
`MediaProgress.svelte` is one 14 px row. In the panel it sits under the artist, in the media text column beside the cover art, so the art still sets the row height and the page keeps the heights in section 3.2. The row is the elapsed time (11 px, tabular, 60% white), the bar, and the length. The bar is a 14 px hit area with a 3 px track at 16% white and a white fill scaled from its left edge. In the lyrics layout it is a row of its own in the 250 px left column, below the title and art block and above the transport (section 3.3), so the column is 174 px tall.

- The elapsed time is the last sampled position, extrapolated while a track plays (`positionAt` in `lib/lyrics.ts`), and clamped to the length. A timer ticks 4 times a second, and only while the panel shows the Media page and a track plays. Nothing ticks otherwise.
- A click or a drag seeks (`media_seek`), and the bar shows the target until the next media update, or for 2 s if no update comes. The keyboard seeks 5 s with the arrow keys. The bar is `role="slider"` with `aria-valuenow`, `aria-valuemin`, `aria-valuemax` and an `aria-valuetext` of "elapsed of length". It takes a 2 px focus ring at 70% white, offset 2 px.
- A drag keeps its pointer captured, so leaving the bar does not lose it. A drag that starts on the bar does not turn the page: `swipeStart` skips it, as it skips sliders.
- The row is hidden when the length is unknown (zero, a live stream or no session), so nothing shows a bar that cannot seek.
- Only `transform` animates. The fill eases between ticks, and a drag has no transition. Reduced motion has no transition at all, so the fill jumps.

### Lyrics transport
Three round buttons: skip 44 px, play 52 px, glyphs 28 px and 32 px, 14 px gap, centred under the progress row in the left column (section 3.3).

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
| 12 | The notification text started at the badge's edge, with no gap between them. | Keep an 8 px gap between a compact pill's badge and its text column, and mirror it on the right so the text stays centred. |
| 13 | Svelte drops a space at the start of an element, so a title and body joined with `<span> {body}</span> ` ran together ("Lunch at noon?Bring"). | Emit a separator as its own expression, such as `{" "}`, not as a leading space inside a span. |
| 14 | A form copied its seed once (`let form = {...seed}`), so a second "add" for another day, while the form was open, kept the first day. | When a child copies a prop at creation, remount it with `{#key}` on a counter that every open bumps. |
| 15 | A class name the island already uses (`.frame`) matched the island's own wrapper in a test query, so the first measurements read the wrong box. | Give page elements their own class names, and measure the element you mean (check `.closest()` or the component). |

## 9. Accessibility and reduced motion

- Reduced motion is read in two places: the CSS media query `(prefers-reduced-motion: reduce)`, and a
  module-level `matchMedia` check in `Island.svelte`, `ControlsPage.svelte` and `RollingClock.svelte`.
  The JavaScript check is read once when the module loads, so a change while the app runs needs a
  restart. Keep both in step when you add a motion.
- Under reduced motion: shapes and pages change by fade only (150 ms) with no transform, the equaliser
  bars are static, the HUD bar, the battery glyph, the media progress fill and the ring jump instead of gliding, the timer digits crossfade over
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
| `pages` | media, controls, timer, stats | Panel pages and their order. One page hides the pager. Old files without `stats` keep the page off until it is turned on. |
| `controls` | eight ids | Quick controls and their order (sliders first, at most seven tiles). |
| `lyrics` | on | Synced lyrics layout. |
| `confirmLock` | on | First tap arms Lock for 3 s. |
| `hud` | on | Volume and brightness HUD. |
| `timerSound` | on | Beep when a countdown ends. |
| `notifications` (`enabled`, `deniedApps`) | on, no apps denied | Shows other apps' Windows toasts in the notification pill. Settings > Notifications lists the apps and turns each on or off. |
| `idleClock` | on | Shows HH:MM, dim, in the idle pill. |
| `battery` (`enabled`, `chargeAlerts`, `lowBatteryAlerts`) | on, on, on | Shows the battery pill for the charger, full charge and low-battery warnings. Settings > Battery. A PC with no battery shows nothing. |
| `stats` (`enabled`) | on | Lets the Stats page show and be sampled. Settings > Stats. Off, the page is hidden and nothing is read. |
| `focusMinutes`, `shortBreakMinutes`, `longBreakMinutes` | 25, 5, 15 | Pomodoro lengths. |
| `hotkeys` (`enabled`, `playPause`, `nextTrack`, `previousTrack`, `timerToggle`) | off; Ctrl+Alt+Space, Ctrl+Alt+Right, Ctrl+Alt+Left, Ctrl+Alt+T | Global keys for play or pause, next, previous and the timer. Settings > Hotkeys. Nothing is registered while off, and an empty box leaves its action unbound. A conflict is shown under the action. |
| `calendar` (`enabled`, `leadMinutes`, `allDayHour`) | on, 10, 9 | Reminders for the user's own events, and the Calendar page. Settings > Calendar sets the lead time (5, 10, 15, 30 or 60 min) and the hour all-day events are announced at. Events themselves are in `events.json`. Settings is the only place to change them: the island's Calendar page opens it for "Add note". |

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

- The Stats page shows CPU, memory, network and the system drive only. CPU temperature and GPU
  are not shown: no reliable reading comes without admin rights or a vendor driver.
- The real Tauri window is not part of the browser harness. Click-through, the window's placement and
  the Settings window's appearance are checked in `tauri dev`.
- The notification pill takes the cursor like the HUD does, because the window hit-tests every pill
  by its size. Its own element has `pointer-events: none`, and nothing is wired to it, so a click
  does nothing. The pill never shows an icon, only the app's initial.
- The battery pill was checked in the browser harness, which drives the `battery://show` and
  `island://state` events. A real plug or unplug on the Tauri window is not checked.
- The calendar reminder pill and page were checked in the browser harness, which drives
  `calendar://show`, `calendar://update` and `island://state`. The reminder times themselves (a real
  event firing at its start) are checked in `tauri dev`, not here.
- The lyrics layout (title block, progress row, transport, lyric stack and the fit) was measured in the
  browser harness at rest and with a long line, and not in the Tauri window. A line too long for 700 x 240
  even at the 0.7 floor does not fit: a 300-character line needs about eight rows there, and it overlaps
  its neighbours rather than clipping. That is the limit of the rule, not a bug to hide.
- The media progress line's seek, the calendar snooze strip, and the Settings form's length and reminder chips were
  not measured in the browser harness or in the Tauri window. Their sizes come from the code and this file, and
  the snooze strip's layout (a 102 px body, with the strip in the bottom 30 px) needs a check on a real screen.
- The Hotkeys section has no harness check. Its conflict line depends on what other apps hold.
- The island month grid was checked in the browser harness only. The harness clicked the days and the
  arrows, and its "Add note" recorded the call. Not checked in the Tauri window: real pointer hits through
  the notch, the Settings window opening and scrolling to the calendar on a click, and whether the island
  collapses while Settings takes focus.
- The idle clock has no calendar line. An event within the hour would need the idle pill to show a
  second line or a wider chip, which would break the centring and the size in section 10, so it was
  left out.
- Timer digits are measured by line box, not by ink. A font with a tall or short cap height can look
  a pixel off-centre. Adjust `line-height` on the element, not the padding, if that happens.
- The Settings month grid was checked in the browser harness only, at 355 and 560 px. Its reduced-motion
  branch is not exercised there, because the harness cannot set `prefers-reduced-motion`. A day with several
  notes shows a colour and no count, so the brief's optional dot count is not drawn.
