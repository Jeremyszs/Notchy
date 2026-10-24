//! Overlay window setup and cursor-driven click-through. Outside the pill the
//! window ignores the mouse, so the apps underneath stay clickable.

use std::ffi::c_void;
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetWindowLongPtrW, GetWindowRect, GWL_EXSTYLE, SetWindowLongPtrW, STYLESTRUCT,
    WM_STYLECHANGING, WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
};

use crate::state::{self, IslandState, SharedIsland};
use crate::config::{self, SharedConfig};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

// Must match the window size in tauri.conf.json.
const WINDOW_WIDTH: f64 = 720.0;
const WINDOW_HEIGHT: f64 = 240.0;
// Slack around the pill so a fast cursor flick onto it still registers.
const HIT_MARGIN: f64 = 6.0;
const POLL_INTERVAL: Duration = Duration::from_millis(16);
// A hidden island cannot be hovered, so checking it less often costs nothing visible.
const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(100);
// The 5 px tucked sliver is too thin to aim at, so its hit area is padded to this height.
const TUCKED_MIN_HIT_HEIGHT: f64 = 10.0;
// A cursor sweeping across an app's tab strip should not pop the island open.
const TUCKED_HOVER_DWELL: Duration = Duration::from_millis(250);
const BASE_DPI: f64 = 96.0;

/// Configures the main window, shows it at top-center, and starts the cursor watcher.
/// Runs from the setup hook, while the window is still hidden.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .expect("tauri.conf.json must declare a window labelled \"main\"");

    let hwnd = window.hwnd()?;
    // The guard must be installed before anything that touches the ex-style. tao
    // rewrites GWL_EXSTYLE from its cached flags on every flag change (show, and each
    // set_ignore_cursor_events toggle from the cursor thread), wiping any one-off bits
    // set earlier. Its toggles also run asynchronously on the window thread, so
    // re-applying from the cursor thread would race them. Intercepting the write is the
    // only ordering-independent hook.
    install_style_guard(hwnd)?;
    position_top_center(&window)?;
    window.set_ignore_cursor_events(true)?;
    window.show()?;
    // Set once more after show() so the style is correct before the first hover edge.
    apply_overlay_styles(hwnd);

    let island = app.state::<SharedIsland>().inner().clone();
    spawn_cursor_watch(app.clone(), window, hwnd, island);
    Ok(())
}

/// Extended-style bits the overlay needs on top of whatever tao wants.
/// WS_EX_TOOLWINDOW keeps the overlay off the taskbar and Alt+Tab. WS_EX_NOACTIVATE
/// stops clicks on it from stealing focus.
const OVERLAY_EX_ADD: u32 = WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
/// WS_EX_APPWINDOW forces a taskbar button and overrides WS_EX_TOOLWINDOW, so it must go.
const OVERLAY_EX_REMOVE: u32 = WS_EX_APPWINDOW.0;

/// Merges the overlay bits into the current ex-style. Merging keeps WS_EX_LAYERED and
/// WS_EX_TRANSPARENT, which transparency and click-through depend on.
fn apply_overlay_styles(hwnd: HWND) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let next = (style & !OVERLAY_EX_REMOVE) | OVERLAY_EX_ADD;
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next as isize);
    }
}

/// Subclasses the window so every WM_STYLECHANGING for GWL_EXSTYLE has the overlay bits
/// merged into the new value before the style is committed. This holds no matter who
/// writes the style or when.
fn install_style_guard(hwnd: HWND) -> tauri::Result<()> {
    let ok = unsafe { SetWindowSubclass(hwnd, Some(style_guard), STYLE_GUARD_ID, 0) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(std::io::Error::other("SetWindowSubclass failed").into())
    }
}

const STYLE_GUARD_ID: usize = 0x6E6F_7463; // "notc"

unsafe extern "system" fn style_guard(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_STYLECHANGING && wparam.0 as isize == GWL_EXSTYLE.0 as isize {
        // lparam points at a STYLESTRUCT whose styleNew is the value about to be set.
        let styles = lparam.0 as *mut STYLESTRUCT;
        if !styles.is_null() {
            unsafe {
                let new = &mut (*styles).styleNew;
                *new = (*new & !OVERLAY_EX_REMOVE) | OVERLAY_EX_ADD;
            }
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Centers the window horizontally on the primary monitor, flush with its top edge.
fn position_top_center(window: &WebviewWindow) -> tauri::Result<()> {
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(());
    };

    // Sizing in logical units lets Tauri apply the DPI scale; centering then has to
    // use the same scale so the window's physical width matches.
    let scale = monitor.scale_factor();
    window.set_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT))?;

    let origin = monitor.position();
    let window_width = (WINDOW_WIDTH * scale).round() as i32;
    let monitor_width = monitor.size().width as i32;
    let x = origin.x + (monitor_width - window_width) / 2;
    window.set_position(PhysicalPosition::new(x, origin.y))
}

/// Polls the cursor and toggles click-through only when it crosses the pill edge.
/// Edge-triggered, so an idle overlay does almost no work.
fn spawn_cursor_watch(app: AppHandle, window: WebviewWindow, hwnd: HWND, island: SharedIsland) {
    // HWND wraps a raw pointer and is not Send, so it crosses the thread boundary as an integer.
    let hwnd_raw = hwnd.0 as isize;

    thread::spawn(move || {
        let hwnd = HWND(hwnd_raw as *mut c_void);
        let mut inside = false;
        let mut interactive = false;
        let mut tucked = TuckedGate::default();
        let mut interval = POLL_INTERVAL;

        loop {
            thread::sleep(interval);

            let Some(cursor) = cursor_position() else {
                continue;
            };
            let Some(rect) = window_rect(hwnd) else {
                continue;
            };
            let scale = dpi_scale(hwnd);
            let (state, dragging) = {
                let machine = state::lock(&island);
                (machine.current(), machine.file_drag_active())
            };
            interval = if state == IslandState::Hidden {
                HIDDEN_POLL_INTERVAL
            } else {
                POLL_INTERVAL
            };

            let on_pill =
                hit_box(state).is_some_and(|size| pill_contains(rect, scale, size, cursor));
            let held = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } < 0;
            let files = config::lock(app.state::<SharedConfig>().inner()).shelf.files;
            let armed = drag_armed(state, files, held, rect, scale, cursor);
            if dragging && !armed {
                state::commit(&app, &island, |machine| machine.set_file_drag(false));
                let _ = app.emit("shelf://drag", false);
            }
            // Holding a button only arms hit-testing; a confirmed file enter expands.
            let hit = if held {
                tucked.reset();
                inside && on_pill
            } else if state == IslandState::Tucked {
                tucked.admit(on_pill, Instant::now())
            } else {
                tucked.reset();
                on_pill
            };
            let takes_cursor = armed || hit;
            if takes_cursor != interactive {
                interactive = takes_cursor;
                let _ = window.set_ignore_cursor_events(!interactive);
            }
            if hit == inside {
                continue;
            }
            inside = hit;

            let _ = app.emit("island://hover", hit);
            state::commit(&app, &island, |machine| machine.set_hovered(hit));
        }
    });
}

fn drag_armed(state: IslandState, files: bool, held: bool, rect: RECT, scale: f64, cursor: POINT) -> bool {
    if !files || !held || state == IslandState::Hidden { return false; }
    let (w, h) = state.pill_size();
    // pill_contains already adds HIT_MARGIN.
    pill_contains(rect, scale, (w + 36.0, h + 18.0), cursor)
}

/// Logical size of the hit area for a state, or None when the island takes no hover.
fn hit_box(state: IslandState) -> Option<(f64, f64)> {
    match state {
        IslandState::Hidden => None,
        IslandState::Tucked => {
            let (w, h) = state.pill_size();
            Some((w, h.max(TUCKED_MIN_HIT_HEIGHT)))
        }
        _ => Some(state.pill_size()),
    }
}

/// Holds back the hover edge while the cursor is on the tucked sliver until it has
/// dwelled there. Leaving the sliver takes effect at once.
#[derive(Default)]
struct TuckedGate {
    entered: Option<Instant>,
}

impl TuckedGate {
    fn admit(&mut self, on_pill: bool, now: Instant) -> bool {
        if !on_pill {
            self.entered = None;
            return false;
        }
        let since = *self.entered.get_or_insert(now);
        now.duration_since(since) >= TUCKED_HOVER_DWELL
    }

    fn reset(&mut self) {
        self.entered = None;
    }
}

/// Hit-tests a cursor in screen pixels against the pill. The pill is centered in
/// the window and top-aligned, matching the layout in Island.svelte.
fn pill_contains(rect: RECT, scale: f64, (pill_w, pill_h): (f64, f64), cursor: POINT) -> bool {
    let center_x = f64::from(rect.left + rect.right) / 2.0;
    let half_width = (pill_w / 2.0 + HIT_MARGIN) * scale;
    let top = f64::from(rect.top);
    let bottom = top + (pill_h + HIT_MARGIN) * scale;

    let x = f64::from(cursor.x);
    let y = f64::from(cursor.y);
    (center_x - half_width..=center_x + half_width).contains(&x)
        && (top..=bottom).contains(&y)
}

fn cursor_position() -> Option<POINT> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok().map(|_| point)
}

// Read through Win32 instead of the Tauri window API, which round-trips to the event
// loop. Doing that at 60 Hz would add contention for no benefit.
fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut rect = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut rect) }.ok().map(|_| rect)
}

fn dpi_scale(hwnd: HWND) -> f64 {
    match unsafe { GetDpiForWindow(hwnd) } {
        0 => 1.0,
        dpi => f64::from(dpi) / BASE_DPI,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_arming_needs_button_files_visible_and_dpi_scaled_zone() {
        let rect = RECT { left: 0, top: 0, right: 1000, bottom: 480 };
        let at = |x, y| POINT { x, y };
        assert!(drag_armed(IslandState::Idle, true, true, rect, 2.0, at(668, 108)));
        assert!(!drag_armed(IslandState::Idle, true, true, rect, 2.0, at(669, 108)));
        assert!(!drag_armed(IslandState::Idle, true, true, rect, 2.0, at(500, 109)));
        assert!(!drag_armed(IslandState::Hidden, true, true, rect, 2.0, at(500, 0)));
        assert!(!drag_armed(IslandState::Idle, false, true, rect, 2.0, at(500, 0)));
        assert!(!drag_armed(IslandState::Idle, true, false, rect, 2.0, at(500, 0)));
    }

    #[test]
    fn hidden_takes_no_hover() {
        assert_eq!(hit_box(IslandState::Hidden), None);
    }

    #[test]
    fn tucked_hit_area_is_padded_to_a_usable_height() {
        assert_eq!(hit_box(IslandState::Tucked), Some((64.0, 10.0)));
        assert_eq!(hit_box(IslandState::Idle), Some((120.0, 30.0)));
    }

    #[test]
    fn tucked_hover_waits_for_the_dwell() {
        let start = Instant::now();
        let mut gate = TuckedGate::default();
        let at = |ms| start + Duration::from_millis(ms);

        assert!(!gate.admit(true, at(0)));
        assert!(!gate.admit(true, at(249)));
        assert!(gate.admit(true, at(250)));
    }

    #[test]
    fn leaving_the_sliver_restarts_the_dwell() {
        let start = Instant::now();
        let mut gate = TuckedGate::default();
        let at = |ms| start + Duration::from_millis(ms);

        gate.admit(true, at(0));
        assert!(!gate.admit(false, at(100)));
        assert!(!gate.admit(true, at(200)));
        assert!(!gate.admit(true, at(449)));
        assert!(gate.admit(true, at(450)));
    }

    #[test]
    fn reset_clears_a_pending_dwell() {
        let start = Instant::now();
        let mut gate = TuckedGate::default();
        let at = |ms| start + Duration::from_millis(ms);

        gate.admit(true, at(0));
        gate.reset();
        assert!(!gate.admit(true, at(300)));
    }

    #[test]
    fn cursor_is_tested_against_the_pill_with_margin() {
        // A 1000 px wide window at x=0; a 120 px pill centered at x=500, top at y=0.
        let rect = RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 200,
        };
        let pill = (120.0, 30.0);
        let at = |x, y| POINT { x, y };

        assert!(pill_contains(rect, 1.0, pill, at(500, 0)));
        assert!(pill_contains(rect, 1.0, pill, at(500 + 66, 36)));
        assert!(!pill_contains(rect, 1.0, pill, at(500 + 67, 0)));
        assert!(!pill_contains(rect, 1.0, pill, at(500, 37)));
    }
}
