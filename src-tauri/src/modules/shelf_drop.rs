//! Takes files dragged onto the notch. The overlay is click-through, and OLE never reaches its
//! WebView2 child while the overlay is click-through, so no drop reaches the webview. While a button
//! is held in the drop zone, a small top-level window (the catcher) sits under the cursor and takes
//! the drop instead. It has no content and is almost fully transparent.

use std::cell::Cell;
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINTL, RECT, WPARAM};
use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{
    CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, IDropTarget, IDropTarget_Impl,
    RegisterDragDrop, ReleaseStgMedium,
};
use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{Ref, implement, w};

use crate::{modules::shelf, modules::shelf_drag, state};

/// The catcher's place and whether it may show. Locked only for field access, never across a
/// window call: the window calls can wait on the main thread, which may need this lock.
#[derive(Default)]
struct Catcher {
    hwnd: isize,
    /// The screen rectangle it currently covers, or None while hidden.
    shown: Option<(i32, i32, i32, i32)>,
    /// Set by a drag that is not files. Holds the catcher back until the button is released, so
    /// text and links dragged across the top still reach the app underneath.
    suppressed: bool,
}

static CATCHER: Mutex<Catcher> = Mutex::new(Catcher { hwnd: 0, shown: None, suppressed: false });

fn catcher() -> std::sync::MutexGuard<'static, Catcher> {
    CATCHER.lock().unwrap_or_else(|poison| poison.into_inner())
}

unsafe extern "system" fn catcher_proc(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, message, wp, lp) }
}

/// Creates the catcher, owned by the overlay so it stays above it, and registers its drop target.
/// Runs on the main thread, which pumps the window's messages and owns OLE.
pub fn create(app: &AppHandle, owner: HWND) -> windows::core::Result<()> {
    let class = w!("NotchyShelfDrop");
    unsafe {
        let instance = GetModuleHandleW(None).ok().map(|module| module.into());
        let wc = WNDCLASSW {
            hInstance: instance.unwrap_or_default(),
            lpfnWndProc: Some(catcher_proc),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
            class,
            w!(""),
            WS_POPUP,
            0,
            0,
            1,
            1,
            Some(owner),
            None,
            instance,
            None,
        )?;
        // Alpha 1 of 255 keeps the window invisible but hit-testable. A plain transparent window
        // would be skipped by the hit test, and a click-through one would never take the drop.
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA)?;
        let target: IDropTarget = DropTarget { app: app.clone(), accepting: Cell::new(false) }.into();
        RegisterDragDrop(hwnd, &target)?;
        catcher().hwnd = hwnd.0 as isize;
    }
    Ok(())
}

/// Shows the catcher over `zone` while a drop is armed, and hides it otherwise. Called from the
/// cursor watcher on every tick. `held` is the left button, which also ends a suppression.
pub fn update(zone: Option<RECT>, held: bool) {
    let (hwnd, action) = {
        let mut state = catcher();
        if !held {
            state.suppressed = false;
        }
        // A drag out of the shelf is never a drop onto it, so the catcher stays down for it.
        let own = shelf_drag::OWN_DRAG.load(Ordering::SeqCst);
        let want = zone
            .filter(|_| held && !state.suppressed && !own)
            .map(|r| (r.left, r.top, r.right - r.left, r.bottom - r.top));
        let action = match (want, state.shown) {
            (Some(rect), Some(shown)) if rect == shown => None,
            (Some(rect), _) => Some(Some(rect)),
            (None, Some(_)) => Some(None),
            (None, None) => None,
        };
        if action.is_some() {
            state.shown = want;
        }
        (state.hwnd, action)
    };
    let Some(action) = action else { return };
    let hwnd = HWND(hwnd as *mut _);
    match action {
        Some((x, y, w, h)) => unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                x,
                y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        },
        None => unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        },
    }
}

/// Hides the catcher from inside a drop callback, which runs on the main thread.
fn hide() {
    let hwnd = {
        let mut state = catcher();
        state.shown = None;
        state.hwnd
    };
    unsafe {
        let _ = ShowWindow(HWND(hwnd as *mut _), SW_HIDE);
    }
}

fn suppress() {
    catcher().suppressed = true;
}

/// Sets the file drag on the island state and tells the frontend, as the cursor watcher does.
fn set_drag(app: &AppHandle, active: bool) {
    let island = app.state::<state::SharedIsland>();
    state::commit(app, &island, |machine| machine.set_file_drag(active));
    let _ = app.emit("shelf://drag", active);
}

/// The file paths on a drag, or none when the drag carries something other than files.
fn dropped_paths(data: Ref<'_, IDataObject>) -> Vec<PathBuf> {
    let Some(data) = data.as_ref() else {
        return Vec::new();
    };
    let format = FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    };
    let Ok(mut medium) = (unsafe { data.GetData(&format) }) else {
        return Vec::new();
    };
    let hdrop = HDROP(unsafe { medium.u.hGlobal.0 } as _);
    let count = unsafe { DragQueryFileW(hdrop, 0xFFFF_FFFF, None) };
    let mut paths = Vec::with_capacity(count as usize);
    for index in 0..count {
        let len = unsafe { DragQueryFileW(hdrop, index, None) } as usize;
        let mut buffer = vec![0u16; len + 1];
        unsafe { DragQueryFileW(hdrop, index, Some(&mut buffer)) };
        paths.push(PathBuf::from(OsString::from_wide(&buffer[..len])));
    }
    unsafe { ReleaseStgMedium(&mut medium) };
    paths
}

#[implement(IDropTarget)]
struct DropTarget {
    app: AppHandle,
    /// Whether the current drag carries files. Set on enter, read on every over event.
    accepting: Cell<bool>,
}

#[allow(non_snake_case)]
impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        // Our own drag out carries files too, so it is refused before the files are read.
        let own = shelf_drag::OWN_DRAG.load(Ordering::SeqCst);
        let files = !own && !dropped_paths(data).is_empty();
        self.accepting.set(files);
        if own {
            suppress();
            hide();
        } else if files {
            set_drag(&self.app, true);
        } else {
            // Not files: give the zone back to whatever is underneath until the button is up.
            suppress();
            hide();
        }
        unsafe { *effect = if files { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        unsafe { *effect = if self.accepting.get() { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        self.accepting.set(false);
        set_drag(&self.app, false);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = dropped_paths(data);
        self.accepting.set(false);
        set_drag(&self.app, false);
        hide();
        let accepted = !shelf_drag::OWN_DRAG.load(Ordering::SeqCst) && !paths.is_empty();
        if accepted {
            let added = shelf::add_paths(&self.app, paths);
            let _ = self.app.emit("shelf://added", added);
        }
        unsafe { *effect = if accepted { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        Ok(())
    }
}
