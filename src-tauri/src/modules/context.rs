//! Screen awareness. WinEvent hooks report when the foreground window changes or moves,
//! and a worker turns that into two flags: is the foreground window fullscreen, and is it
//! sitting under the notch. Nothing polls. Hooks only post a message; the worker does the
//! Win32 reads.

use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONULL, MONITOR_DEFAULTTOPRIMARY, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CHILDID_SELF, DispatchMessageW, EVENT_OBJECT_CLOAKED, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_UNCLOAKED, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MINIMIZESTART, GWL_STYLE, GetClassNameW,
    GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, IsZoomed, MSG, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WS_CAPTION,
};

use super::{Module, ModuleContext};
use crate::state::{self, IslandState};

const BASE_DPI: f64 = 96.0;
// Location changes arrive in bursts while a window is dragged. Recomputing once per burst
// keeps the worker quiet, and the state still lands within this delay of the last event.
const COALESCE: Duration = Duration::from_millis(50);
// Fullscreen must read true this long before it commits Hidden. Leaving fullscreen never waits.
const FULLSCREEN_CONFIRM: Duration = Duration::from_millis(150);
// Rechecks after an event burst settles. They catch a reading taken mid-transition, such as
// a window still animating into place, when no further event arrives to correct it.
const TRAILING_CHECKS: [Duration; 2] = [Duration::from_millis(120), Duration::from_millis(400)];
// Logical px of slack beyond the Active pill, on each side and below it.
const NOTCH_SIDE_MARGIN: f64 = 12.0;
const NOTCH_BOTTOM_MARGIN: f64 = 10.0;

/// Debug trace file. Restarted once it grows past this, so it never fills the disk.
const TRACE_CAP: u64 = 1024 * 1024;
const TRACE_TITLE_CHARS: usize = 60;

/// Shell surfaces that take the foreground but are not apps: the desktop, the taskbars,
/// Start and search, the Alt+Tab and Task View overlays, and the UWP core window they host.
const SHELL_CLASSES: [&str; 7] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "MultitaskingViewFrame",
    "XamlExplorerHostIslandWindow",
    "Windows.UI.Core.CoreWindow",
];

/// Feeds the hook callbacks into the worker. Callbacks are plain functions with no user
/// data, so the channel has to live in a static.
static EVENTS: OnceLock<Sender<()>> = OnceLock::new();

/// What the foreground window tells the island. Each flag is independent; the state
/// machine decides which one wins.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Presence {
    fullscreen: bool,
    covered: bool,
}

/// The primary monitor, in physical pixels.
struct Screen {
    bounds: RECT,
    scale: f64,
}

/// Plain facts about the foreground window. The Win32 reads fill these in; `classify`
/// decides from them alone so it can be tested without a desktop.
#[derive(Debug, Clone, Copy)]
struct WindowFacts {
    hwnd: HWND,
    /// Visible, not minimized, and not cloaked (for example, on another virtual desktop).
    visible: bool,
    /// DWM reports the window as cloaked.
    cloaked: bool,
    /// Belongs to this process, which is never allowed to tuck or hide the island.
    ours: bool,
    /// One of the shell surfaces in SHELL_CLASSES.
    shell: bool,
    on_primary: bool,
    /// Visible frame, without the invisible resize borders that GetWindowRect includes.
    bounds: RECT,
    /// Raw GWL_STYLE, kept for the debug trace.
    style: u32,
    /// Has a title bar. Maximized windows keep one; fullscreen windows drop it.
    captioned: bool,
    /// WS_MAXIMIZE is set (IsZoomed). Real fullscreen windows are never maximized.
    zoomed: bool,
    /// Raw SHQueryUserNotificationState value, or -1 when the call failed.
    notif: i32,
    /// SHQueryUserNotificationState reports a fullscreen app or presentation.
    system_busy: bool,
}

pub struct ContextModule;

impl Module for ContextModule {
    fn name(&self) -> &'static str {
        "context"
    }

    fn start(&self, ctx: ModuleContext) {
        let (events, inbox) = mpsc::channel();
        // One message up front so the state is right before the first foreground change.
        let _ = events.send(());
        let _ = EVENTS.set(events);
        // Read once here. With NOTCHY_DEBUG unset the worker holds None and never traces.
        let trace = debug_enabled().then(Trace::open);
        thread::spawn(hook_thread);
        thread::spawn(move || worker(ctx, inbox, trace));
    }
}

fn debug_enabled() -> bool {
    std::env::var_os("NOTCHY_DEBUG").is_some_and(|value| value == "1")
}

/// Owns the WinEvent hooks. Out-of-context hooks are delivered as messages to this
/// thread's queue, so it has to pump messages for the life of the app.
fn hook_thread() {
    let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
    // Each range is a separate hook. Ranges cover only the events we use, so the callback
    // is not called for every window event on the desktop.
    let ranges = [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZEEND),
        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
        (EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE),
        (EVENT_OBJECT_CLOAKED, EVENT_OBJECT_UNCLOAKED),
    ];
    for (min, max) in ranges {
        // A failed hook only means that event never arrives; the rest keep working.
        let _ = unsafe { SetWinEventHook(min, max, None, Some(on_event), 0, 0, flags) };
    }

    let mut msg = MSG::default();
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        unsafe { DispatchMessageW(&msg) };
    }
}

/// Runs on the hook thread, so it does one filter and one send. Events for windows other
/// than the foreground one are dropped here, which keeps drags from flooding the worker.
unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _thread: u32,
    _time: u32,
) {
    if id_object != OBJID_WINDOW.0 || id_child != CHILDID_SELF as i32 {
        return;
    }
    let is_foreground = unsafe { GetForegroundWindow() } == hwnd;
    if event != EVENT_SYSTEM_FOREGROUND && !is_foreground {
        return;
    }
    if let Some(events) = EVENTS.get() {
        let _ = events.send(());
    }
}

/// Why a recompute is running. Only an event burst schedules trailing rechecks.
#[derive(Debug, Clone, Copy)]
enum Trigger {
    Event,
    Check,
}

/// Recomputes after each burst of events, runs the rechecks the settle logic asks for, and
/// commits only when the settled presence changed. Sleeps in recv or recv_timeout when idle.
fn worker(ctx: ModuleContext, inbox: Receiver<()>, mut trace: Option<Trace>) {
    let mut settle = Settle::default();

    loop {
        let received = match settle.next_deadline() {
            Some(due) => inbox.recv_timeout(due.saturating_duration_since(Instant::now())),
            None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match received {
            Ok(()) => {
                // Sleep so a burst lands in one pass, then drop the rest of it. This is a
                // throttle, not a busy loop.
                thread::sleep(COALESCE);
                while inbox.try_recv().is_ok() {}
                recompute(&ctx, &mut settle, trace.as_mut(), Trigger::Event);
            }
            Err(RecvTimeoutError::Timeout) => {
                settle.take_due(Instant::now());
                recompute(&ctx, &mut settle, trace.as_mut(), Trigger::Check);
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// One pass: read the foreground window, settle it, trace it, and commit if it changed.
fn recompute(
    ctx: &ModuleContext,
    settle: &mut Settle,
    trace: Option<&mut Trace>,
    trigger: Trigger,
) {
    let now = Instant::now();
    let primary = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
    let screen = primary_screen(primary);
    let facts = foreground_facts(primary);
    let raw = screen.as_ref().map_or(Presence::default(), |screen| {
        classify(facts.as_ref(), screen)
    });

    let committed = settle.observe(raw, now);
    if matches!(trigger, Trigger::Event) {
        settle.after_burst(now);
    }
    if let Some(trace) = trace {
        trace.record(
            trigger,
            facts.as_ref(),
            screen.as_ref(),
            Reading {
                raw,
                settled: settle.committed.unwrap_or_default(),
                committed: committed.is_some(),
            },
        );
    }
    if let Some(presence) = committed {
        state::commit(&ctx.app, &ctx.island, |machine| {
            machine.set_fullscreen(presence.fullscreen);
            machine.set_covered(presence.covered);
        });
    }
}

/// Turns raw readings into the committed presence. Only fullscreen waits: it commits once
/// it has read true for FULLSCREEN_CONFIRM, so a transient frame cannot hide the island.
/// Every other change, including leaving fullscreen, commits at once.
/// Pure: the caller supplies the clock, so the timing can be tested.
#[derive(Debug, Default)]
struct Settle {
    committed: Option<Presence>,
    /// When the current run of fullscreen readings began. None after a non-fullscreen reading.
    fullscreen_since: Option<Instant>,
    /// When the current fullscreen run becomes confirmable. Keeps the worker awake for it.
    confirm_at: Option<Instant>,
    /// Trailing recheck deadlines from the most recent event burst.
    trailing: Vec<Instant>,
}

impl Settle {
    /// Folds in one raw reading. Returns the presence to commit, or None when nothing changed.
    fn observe(&mut self, raw: Presence, now: Instant) -> Option<Presence> {
        let fullscreen = if !raw.fullscreen {
            self.fullscreen_since = None;
            self.confirm_at = None;
            false
        } else if self.committed.is_some_and(|presence| presence.fullscreen) {
            true
        } else {
            let since = match self.fullscreen_since {
                Some(since) => since,
                None => {
                    self.fullscreen_since = Some(now);
                    self.confirm_at = Some(now + FULLSCREEN_CONFIRM);
                    now
                }
            };
            now.saturating_duration_since(since) >= FULLSCREEN_CONFIRM
        };

        let next = Presence {
            fullscreen,
            covered: raw.covered,
        };
        if self.committed == Some(next) {
            return None;
        }
        self.committed = Some(next);
        Some(next)
    }

    /// An event burst replaces the trailing rechecks of the previous burst.
    fn after_burst(&mut self, now: Instant) {
        self.trailing.clear();
        self.trailing
            .extend(TRAILING_CHECKS.iter().map(|delay| now + *delay));
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.trailing.iter().copied().chain(self.confirm_at).min()
    }

    /// Drops the deadlines that have come due. The caller recomputes right after.
    fn take_due(&mut self, now: Instant) {
        self.trailing.retain(|at| *at > now);
        if self.confirm_at.is_some_and(|at| at <= now) {
            self.confirm_at = None;
        }
    }
}

fn primary_screen(monitor: HMONITOR) -> Option<Screen> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
        return None;
    }

    let (mut dpi_x, mut dpi_y) = (0_u32, 0_u32);
    let scale =
        match unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) } {
            Ok(()) if dpi_x > 0 => f64::from(dpi_x) / BASE_DPI,
            _ => 1.0,
        };
    // rcMonitor, not rcWork: a fullscreen window covers the whole monitor, taskbar included.
    Some(Screen {
        bounds: info.rcMonitor,
        scale,
    })
}

/// Reads the foreground window. None means there is no window worth looking at.
fn foreground_facts(primary: HMONITOR) -> Option<WindowFacts> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }

    // DWM's extended frame bounds are the visible frame. GetWindowRect would include the
    // invisible resize borders, so a maximized window would look larger than it is.
    let mut bounds = RECT::default();
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast::<c_void>(),
            size_of::<RECT>() as u32,
        )
    }
    .ok()?;

    let mut pid = 0_u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32)) };

    let mut class = [0_u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut class) };
    let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);

    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
    let notif = unsafe { SHQueryUserNotificationState() };
    let system_busy = matches!(
        notif,
        Ok(state) if state == QUNS_BUSY
            || state == QUNS_RUNNING_D3D_FULL_SCREEN
            || state == QUNS_PRESENTATION_MODE
    );
    let cloaked = is_cloaked(hwnd);
    let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();

    Some(WindowFacts {
        hwnd,
        visible: unsafe { IsWindowVisible(hwnd) }.as_bool()
            && !unsafe { IsIconic(hwnd) }.as_bool()
            && !cloaked,
        cloaked,
        ours: pid == std::process::id(),
        shell: SHELL_CLASSES.contains(&class.as_str()),
        on_primary: unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) } == primary,
        bounds,
        style,
        captioned: style & WS_CAPTION.0 == WS_CAPTION.0,
        zoomed,
        notif: notif.map_or(-1, |state| state.0),
        system_busy,
    })
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0_u32;
    let read = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast::<c_void>(),
            size_of::<u32>() as u32,
        )
    };
    read.is_ok() && cloaked != 0
}

/// Decides what the foreground window means for the island. Pure, so every rule can be
/// tested with plain rectangles.
fn classify(window: Option<&WindowFacts>, screen: &Screen) -> Presence {
    let Some(window) = window else {
        return Presence::default();
    };
    if !window.visible || window.ours || window.shell || !window.on_primary {
        return Presence::default();
    }

    // A fullscreen window covers the whole monitor and has no title bar. A maximized
    // window is never fullscreen, even when it is frameless and reaches every monitor edge.
    // An auto-hidden taskbar lets a restored-from-taskbar frameless window report the full
    // monitor rect while still maximized. Chromium and Electron un-maximize before entering
    // real fullscreen (F11, HTML fullscreen), and games and players are not maximized, so
    // real fullscreen always has zoomed=false. The system's busy state is machine-wide,
    // so it only upgrades a monitor-covering window that keeps a title bar. Alone, it
    // could hide the island because of a game on another monitor.
    let covers_screen = contains(window.bounds, screen.bounds);
    let fullscreen =
        covers_screen && !window.zoomed && (!window.captioned || window.system_busy);
    let covered = overlaps(window.bounds, notch_zone(screen));

    Presence {
        fullscreen,
        covered,
    }
}

/// The part of the primary monitor the notch occupies: the Active pill's width plus a
/// margin, from the top edge down past the pill. Physical pixels.
fn notch_zone(screen: &Screen) -> RECT {
    let (pill_w, pill_h) = IslandState::Active.pill_size();
    let bounds = screen.bounds;
    let center_x = f64::from(bounds.left + bounds.right) / 2.0;
    let half_width = (pill_w / 2.0 + NOTCH_SIDE_MARGIN) * screen.scale;
    let bottom = f64::from(bounds.top) + (pill_h + NOTCH_BOTTOM_MARGIN) * screen.scale;

    RECT {
        left: (center_x - half_width).round() as i32,
        top: bounds.top,
        right: (center_x + half_width).round() as i32,
        bottom: bottom.round() as i32,
    }
}

/// True when `outer` covers all of `inner`.
fn contains(outer: RECT, inner: RECT) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// True when the rectangles share any area. Touching edges do not count.
fn overlaps(a: RECT, b: RECT) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

/// Debug trace for NOTCHY_DEBUG=1. Every recompute goes to stderr and to
/// %TEMP%\notchy-debug.log. The file is restarted at startup if it is already over the cap,
/// and again whenever a write would pass it.
struct Trace {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

impl Trace {
    fn open() -> Self {
        let path = std::env::temp_dir().join("notchy-debug.log");
        if fs::metadata(&path).is_ok_and(|meta| meta.len() > TRACE_CAP) {
            let _ = File::create(&path);
        }
        let mut trace = Self {
            path,
            file: None,
            written: 0,
        };
        trace.reopen();
        eprintln!("notchy: debug trace on, writing {}", trace.path.display());
        trace
    }

    fn reopen(&mut self) {
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = self
            .file
            .as_ref()
            .and_then(|file| file.metadata().ok())
            .map_or(0, |meta| meta.len());
    }

    fn line(&mut self, text: &str) {
        eprintln!("{text}");
        let len = text.len() as u64 + 1;
        if self.written + len > TRACE_CAP {
            self.file = None;
            let _ = File::create(&self.path);
            self.reopen();
        }
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if writeln!(file, "{text}").is_ok() {
            self.written += len;
        }
    }

    fn record(
        &mut self,
        trigger: Trigger,
        facts: Option<&WindowFacts>,
        screen: Option<&Screen>,
        reading: Reading,
    ) {
        let ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis());
        let window = match facts {
            Some(f) => {
                let (class, title) = describe(f.hwnd);
                let title: String = title.chars().take(TRACE_TITLE_CHARS).collect();
                format!(
                    "hwnd=0x{:x} class={class:?} title={title:?} frame={} style=0x{:08x} \
                     zoomed={} cloaked={} visible={} ours={} shell={} on_primary={} \
                     captioned={} notif={} busy={}",
                    f.hwnd.0 as usize,
                    fmt_rect(f.bounds),
                    f.style,
                    f.zoomed,
                    f.cloaked,
                    f.visible,
                    f.ours,
                    f.shell,
                    f.on_primary,
                    f.captioned,
                    f.notif,
                    f.system_busy,
                )
            }
            None => "window=none".to_owned(),
        };
        let monitor = screen.map_or_else(|| "none".to_owned(), |s| fmt_rect(s.bounds));
        let Reading {
            raw,
            settled,
            committed,
        } = reading;
        let line = format!(
            "{ms} {trigger:?} {window} monitor={monitor} raw=fullscreen:{} covered:{} \
             settled=fullscreen:{} covered:{} committed={committed}",
            raw.fullscreen, raw.covered, settled.fullscreen, settled.covered,
        );
        self.line(&line);
    }
}

/// One recompute as the trace reports it: the raw reading, the presence it settled to,
/// and whether that settled presence was committed.
#[derive(Clone, Copy)]
struct Reading {
    raw: Presence,
    settled: Presence,
    committed: bool,
}

/// Class name and title. Only the trace reads these, so the normal path never pays for them.
fn describe(hwnd: HWND) -> (String, String) {
    let mut class = [0_u16; 256];
    let class_len = unsafe { GetClassNameW(hwnd, &mut class) }.max(0) as usize;
    let mut title = [0_u16; 256];
    let title_len = unsafe { GetWindowTextW(hwnd, &mut title) }.max(0) as usize;
    (
        String::from_utf16_lossy(&class[..class_len]),
        String::from_utf16_lossy(&title[..title_len]),
    )
}

fn fmt_rect(r: RECT) -> String {
    format!("({},{},{},{})", r.left, r.top, r.right, r.bottom)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRIMARY: RECT = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };

    fn screen() -> Screen {
        Screen {
            bounds: PRIMARY,
            scale: 1.0,
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT {
            left,
            top,
            right,
            bottom,
        }
    }

    /// A visible, captioned, primary-monitor app window at `bounds`.
    fn app(bounds: RECT) -> WindowFacts {
        WindowFacts {
            hwnd: HWND(std::ptr::null_mut()),
            visible: true,
            cloaked: false,
            ours: false,
            shell: false,
            on_primary: true,
            bounds,
            style: WS_CAPTION.0,
            captioned: true,
            zoomed: false,
            notif: 0,
            system_busy: false,
        }
    }

    fn borderless(bounds: RECT) -> WindowFacts {
        WindowFacts {
            captioned: false,
            style: 0,
            ..app(bounds)
        }
    }

    #[test]
    fn no_foreground_window_is_neither() {
        assert_eq!(classify(None, &screen()), Presence::default());
    }

    #[test]
    fn invisible_window_is_ignored() {
        let hidden = WindowFacts {
            visible: false,
            ..borderless(PRIMARY)
        };
        assert_eq!(classify(Some(&hidden), &screen()), Presence::default());
    }

    #[test]
    fn own_window_is_ignored() {
        let ours = WindowFacts {
            ours: true,
            ..borderless(PRIMARY)
        };
        assert_eq!(classify(Some(&ours), &screen()), Presence::default());
    }

    #[test]
    fn shell_surface_is_ignored() {
        let shell = WindowFacts {
            shell: true,
            ..borderless(PRIMARY)
        };
        assert_eq!(classify(Some(&shell), &screen()), Presence::default());
    }

    #[test]
    fn window_on_another_monitor_is_ignored() {
        let elsewhere = WindowFacts {
            on_primary: false,
            ..borderless(PRIMARY)
        };
        assert_eq!(classify(Some(&elsewhere), &screen()), Presence::default());
    }

    #[test]
    fn window_away_from_the_notch_is_neither() {
        let window = app(rect(200, 200, 1200, 900));
        assert_eq!(classify(Some(&window), &screen()), Presence::default());
    }

    #[test]
    fn maximized_window_is_covered_not_fullscreen() {
        // Work area with a bottom taskbar.
        let window = app(rect(0, 0, 1920, 1040));
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: false,
                covered: true
            }
        );
    }

    #[test]
    fn maximized_window_with_auto_hidden_taskbar_is_covered_not_fullscreen() {
        // Reaches every monitor edge, but keeps its title bar.
        let window = app(PRIMARY);
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: false,
                covered: true
            }
        );
    }

    #[test]
    fn borderless_full_monitor_is_fullscreen() {
        let window = borderless(PRIMARY);
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: true,
                covered: true
            }
        );
    }

    /// A frameless, maximized window whose frame matches the monitor. This is the
    /// restored-from-taskbar case from the user's debug log.
    fn zoomed_borderless(bounds: RECT) -> WindowFacts {
        WindowFacts {
            zoomed: true,
            ..borderless(bounds)
        }
    }

    #[test]
    fn zoomed_frameless_full_monitor_is_covered_not_fullscreen() {
        let window = zoomed_borderless(PRIMARY);
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: false,
                covered: true
            }
        );
    }

    #[test]
    fn zoomed_frameless_taskbar_height_frame_is_covered_not_fullscreen() {
        // The maximized frame from the log: 1198 px tall on a 1200 px monitor.
        let window = zoomed_borderless(rect(0, 0, 1920, 1198));
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: false,
                covered: true
            }
        );
    }

    #[test]
    fn zoomed_frameless_full_monitor_is_not_fullscreen_even_when_system_is_busy() {
        // A maximized window never reads as fullscreen, even with the busy signal set.
        let window = WindowFacts {
            system_busy: true,
            ..zoomed_borderless(PRIMARY)
        };
        assert!(!classify(Some(&window), &screen()).fullscreen);
    }

    #[test]
    fn unmaximized_frameless_full_monitor_is_fullscreen() {
        // Borderless game or F11: covers the monitor, not maximized.
        let window = WindowFacts {
            zoomed: false,
            ..borderless(PRIMARY)
        };
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: true,
                covered: true
            }
        );
    }

    #[test]
    fn borderless_window_one_pixel_short_is_not_fullscreen() {
        let window = borderless(rect(0, 0, 1920, 1079));
        assert_eq!(
            classify(Some(&window), &screen()),
            Presence {
                fullscreen: false,
                covered: true
            }
        );
    }

    #[test]
    fn busy_system_upgrades_a_captioned_full_monitor_window() {
        let window = WindowFacts {
            system_busy: true,
            ..app(PRIMARY)
        };
        assert!(classify(Some(&window), &screen()).fullscreen);
    }

    #[test]
    fn busy_system_alone_does_not_hide_a_small_window() {
        let window = WindowFacts {
            system_busy: true,
            ..app(rect(200, 200, 1200, 900))
        };
        assert!(!classify(Some(&window), &screen()).fullscreen);
    }

    #[test]
    fn window_beside_the_notch_zone_is_not_covered() {
        // The zone spans x 848..1072 at 100% scale; this window starts at its right edge.
        let window = app(rect(1072, 0, 1500, 600));
        assert!(!classify(Some(&window), &screen()).covered);
    }

    #[test]
    fn window_just_inside_the_notch_zone_is_covered() {
        let window = app(rect(1071, 0, 1500, 600));
        assert!(classify(Some(&window), &screen()).covered);
    }

    #[test]
    fn window_below_the_notch_zone_is_not_covered() {
        // The zone ends at y=40 at 100% scale.
        assert!(!classify(Some(&app(rect(0, 40, 1920, 1000))), &screen()).covered);
        assert!(classify(Some(&app(rect(0, 39, 1920, 1000))), &screen()).covered);
    }

    #[test]
    fn notch_zone_scales_with_dpi() {
        // At 150% the zone is 60 px tall and 336 px wide, centered on x=1440.
        let scaled = Screen {
            bounds: rect(0, 0, 2880, 1620),
            scale: 1.5,
        };
        assert!(!classify(Some(&app(rect(0, 60, 2880, 1000))), &scaled).covered);
        assert!(classify(Some(&app(rect(0, 59, 2880, 1000))), &scaled).covered);
        assert!(classify(Some(&app(rect(1607, 0, 2000, 300))), &scaled).covered);
        assert!(!classify(Some(&app(rect(1608, 0, 2000, 300))), &scaled).covered);
    }

    #[test]
    fn containment_and_overlap_are_strict_at_edges() {
        assert!(contains(PRIMARY, PRIMARY));
        assert!(!contains(rect(0, 0, 1919, 1080), PRIMARY));
        assert!(!overlaps(rect(0, 0, 10, 10), rect(10, 0, 20, 10)));
        assert!(overlaps(rect(0, 0, 11, 10), rect(10, 0, 20, 10)));
    }

    fn fullscreen() -> Presence {
        Presence {
            fullscreen: true,
            covered: true,
        }
    }

    fn windowed() -> Presence {
        Presence {
            fullscreen: false,
            covered: true,
        }
    }

    /// `base` moved forward by `ms` milliseconds.
    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn transient_fullscreen_that_flips_back_never_commits_hidden() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        let mut commits = vec![settle.observe(windowed(), t0)];
        // Fullscreen flickers on and off well inside the confirmation window, twice.
        commits.push(settle.observe(fullscreen(), at(t0, 10)));
        commits.push(settle.observe(windowed(), at(t0, 100)));
        commits.push(settle.observe(fullscreen(), at(t0, 200)));
        commits.push(settle.observe(windowed(), at(t0, 260)));
        // A trailing recheck that lands in the middle of a run must not confirm it either.
        commits.push(settle.observe(fullscreen(), at(t0, 300)));
        commits.push(settle.observe(windowed(), at(t0, 340)));

        assert!(
            commits.into_iter().flatten().all(|p| !p.fullscreen),
            "a fullscreen reading that did not last must never commit Hidden"
        );
    }

    #[test]
    fn stable_fullscreen_commits_after_the_confirmation_window() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        assert_eq!(settle.observe(windowed(), t0), Some(windowed()));

        assert_eq!(settle.observe(fullscreen(), at(t0, 10)), None);
        assert_eq!(settle.next_deadline(), Some(at(t0, 160)));
        // A recheck before the window has passed is not enough.
        assert_eq!(settle.observe(fullscreen(), at(t0, 100)), None);
        assert_eq!(
            settle.observe(fullscreen(), at(t0, 160)),
            Some(fullscreen())
        );
        // Already committed: further fullscreen readings change nothing.
        assert_eq!(settle.observe(fullscreen(), at(t0, 400)), None);
    }

    #[test]
    fn leaving_fullscreen_commits_immediately() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        settle.observe(fullscreen(), t0);
        settle.observe(fullscreen(), at(t0, 150));
        assert_eq!(settle.observe(windowed(), at(t0, 500)), Some(windowed()));
    }

    #[test]
    fn covered_changes_are_not_held_back_by_a_pending_fullscreen() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        settle.observe(windowed(), t0);
        let uncovered_fullscreen = Presence {
            fullscreen: true,
            covered: false,
        };
        // The covered flag commits right away; fullscreen waits for its confirmation.
        assert_eq!(
            settle.observe(uncovered_fullscreen, at(t0, 10)),
            Some(Presence {
                fullscreen: false,
                covered: false
            })
        );
    }

    #[test]
    fn event_burst_coalesces_trailing_rechecks() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        settle.after_burst(t0);
        assert_eq!(settle.next_deadline(), Some(at(t0, 120)));

        // A newer burst replaces the old schedule; the 120 ms check from t0 is gone.
        settle.after_burst(at(t0, 100));
        assert_eq!(settle.next_deadline(), Some(at(t0, 220)));
        settle.take_due(at(t0, 130));
        assert_eq!(settle.next_deadline(), Some(at(t0, 220)));
        settle.take_due(at(t0, 230));
        assert_eq!(settle.next_deadline(), Some(at(t0, 500)));
        settle.take_due(at(t0, 500));
        assert_eq!(settle.next_deadline(), None);
    }

    #[test]
    fn trailing_rechecks_fire_in_order_and_stop() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        settle.after_burst(t0);
        settle.take_due(at(t0, 120));
        assert_eq!(settle.next_deadline(), Some(at(t0, 400)));
        settle.take_due(at(t0, 400));
        assert_eq!(settle.next_deadline(), None);
    }

    #[test]
    fn pending_confirmation_survives_a_newer_burst() {
        let mut settle = Settle::default();
        let t0 = Instant::now();
        settle.observe(windowed(), t0);
        settle.observe(fullscreen(), at(t0, 10));
        settle.after_burst(at(t0, 50));
        // The confirmation is still due at 160 ms even though the burst rescheduled trailing checks.
        assert_eq!(settle.next_deadline(), Some(at(t0, 160)));
    }
}
