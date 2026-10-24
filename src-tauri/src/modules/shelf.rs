//! Memory-only copied text and file paths. Only previews cross the IPC boundary.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

use super::shelf_drag;
use super::{Module, ModuleContext};
use crate::{config, state};

const CAP: usize = 20;
const UNICODE_TEXT: u32 = 13;

/// The two modes of the page. The wire names match `ShelfItem.kind` in the frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    File,
}

#[derive(Clone, PartialEq, Eq)]
enum Content {
    Text(String),
    File(PathBuf),
}

impl Content {
    fn kind(&self) -> Kind {
        match self {
            Self::Text(_) => Kind::Text,
            Self::File(_) => Kind::File,
        }
    }
}

struct Item {
    id: u64,
    content: Content,
    pinned: bool,
    at_ms: u64,
    order: u64,
    identity: Option<u64>,
    folder: bool,
}

#[derive(Default)]
pub struct Shelf {
    items: Vec<Item>,
    serial: u64,
}

impl Shelf {
    fn add(&mut self, content: Content, at_ms: u64) -> bool {
        self.add_identified(content, at_ms, None)
    }

    fn add_identified(&mut self, content: Content, at_ms: u64, identity: Option<u64>) -> bool {
        self.serial += 1;
        let order = self.serial;
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.content == content && item.identity == identity)
        {
            item.at_ms = at_ms;
            item.order = order;
        } else {
            if self.items.len() == CAP {
                let oldest = self
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| !item.pinned)
                    .min_by_key(|(_, item)| item.order)
                    .map(|(index, _)| index);
                let Some(index) = oldest else { return false };
                self.items.remove(index);
            }
            self.items.push(Item {
                id: order,
                content,
                pinned: false,
                at_ms,
                order,
                identity,
                folder: false,
            });
        }
        self.sort();
        true
    }

    fn text(&mut self, text: &str, at_ms: u64) -> bool {
        if text.trim().is_empty() || text.chars().count() > 20_000 {
            return false;
        }
        // Distinguish long copies sharing a prefix without retaining their discarded text.
        let mut identity = DefaultHasher::new();
        text.hash(&mut identity);
        self.add_identified(
            Content::Text(text.chars().take(2000).collect()),
            at_ms,
            Some(identity.finish()),
        )
    }

    fn sort(&mut self) {
        self.items.sort_by_key(|item| {
            (
                std::cmp::Reverse(item.pinned),
                std::cmp::Reverse(item.order),
            )
        });
    }

    fn pin(&mut self, id: u64, pinned: bool) {
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.pinned = pinned;
        }
        self.sort();
    }

    fn promote(&mut self, id: u64, at_ms: u64) {
        self.serial += 1;
        if let Some(item) = self.items.iter_mut().find(|item| item.id == id) {
            item.at_ms = at_ms;
            item.order = self.serial;
        }
        self.sort();
    }

    /// Removes the unpinned items of one mode. The other mode and every pinned item stay.
    fn clear(&mut self, kind: Kind) {
        self.items
            .retain(|item| item.pinned || item.content.kind() != kind);
    }
    fn disable_clipboard(&mut self) {
        self.items
            .retain(|item| matches!(item.content, Content::File(_)));
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    id: u64,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ext: Option<String>,
    pinned: bool,
    at_ms: u64,
    missing: bool,
}

fn views(shelf: &Shelf) -> Vec<View> {
    shelf
        .items
        .iter()
        .map(|item| {
            let mut view = View {
                id: item.id,
                kind: "text",
                preview: None,
                name: None,
                path: None,
                ext: None,
                pinned: item.pinned,
                at_ms: item.at_ms,
                missing: false,
            };
            match &item.content {
                Content::Text(text) => view.preview = Some(text.chars().take(240).collect()),
                Content::File(path) => {
                    view.kind = "file";
                    view.path = Some(path.to_string_lossy().into_owned());
                    view.name = Some(
                        path.file_name()
                            .unwrap_or(path.as_os_str())
                            .to_string_lossy()
                            .into_owned(),
                    );
                    view.ext = if item.folder || path.is_dir() {
                        None
                    } else {
                        path.extension()
                            .map(|ext| ext.to_string_lossy().into_owned())
                    };
                    view.missing = !path.exists();
                }
            }
            view
        })
        .collect()
}

#[derive(Default)]
pub struct ShelfHandle {
    shelf: Mutex<Shelf>,
    listener: Mutex<Option<Listener>>,
    copied_sequence: AtomicU32,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// Emit under the contents lock so simultaneous commands cannot publish stale lists out of order.
fn publish(app: &AppHandle, shelf: &Shelf) {
    let _ = app.emit("shelf://update", views(shelf));
}

pub struct ShelfModule;
impl Module for ShelfModule {
    fn name(&self) -> &'static str {
        "shelf"
    }
    fn start(&self, ctx: ModuleContext) {
        ctx.app.manage(ShelfHandle::default());
        reapply(&ctx.app);
    }
}

pub fn reapply(app: &AppHandle) {
    let handle = app.state::<ShelfHandle>();
    let mut listener = lock(&handle.listener);
    let settings = config::lock(app.state::<config::SharedConfig>().inner())
        .shelf
        .clone();
    if settings.clipboard && listener.is_none() {
        *listener = Listener::start(app.clone());
    } else if !settings.clipboard {
        if let Some(worker) = listener.take() {
            worker.stop();
        }
        let mut shelf = lock(&handle.shelf);
        shelf.disable_clipboard();
        publish(app, &shelf);
    }
    if !settings.files {
        let island = app.state::<state::SharedIsland>();
        state::commit(app, &island, |machine| machine.set_file_drag(false));
        let _ = app.emit("shelf://drag", false);
    }
}

pub fn shutdown(app: &AppHandle) {
    if let Some(handle) = app.try_state::<ShelfHandle>()
        && let Some(listener) = lock(&handle.listener).take()
    {
        listener.stop();
    }
}

#[tauri::command]
pub fn shelf_list(handle: State<'_, ShelfHandle>) -> Vec<View> {
    views(&lock(&handle.shelf))
}

#[tauri::command]
pub fn shelf_pin(app: AppHandle, handle: State<'_, ShelfHandle>, id: u64, pinned: bool) {
    let mut shelf = lock(&handle.shelf);
    shelf.pin(id, pinned);
    publish(&app, &shelf);
}
#[tauri::command]
pub fn shelf_remove(app: AppHandle, handle: State<'_, ShelfHandle>, id: u64) {
    let mut shelf = lock(&handle.shelf);
    shelf.items.retain(|item| item.id != id);
    publish(&app, &shelf);
}
#[tauri::command]
pub fn shelf_clear(app: AppHandle, handle: State<'_, ShelfHandle>, kind: Kind) {
    let mut shelf = lock(&handle.shelf);
    shelf.clear(kind);
    publish(&app, &shelf);
}
/// Adds dropped paths, newest first, and returns how many were new or moved to the front. Only
/// absolute paths count, and only while the file switch is on.
pub fn add_paths(app: &AppHandle, paths: Vec<PathBuf>) -> usize {
    if !config::lock(app.state::<config::SharedConfig>().inner())
        .shelf
        .files
    {
        return 0;
    }
    let handle = app.state::<ShelfHandle>();
    let mut shelf = lock(&handle.shelf);
    let mut added = 0;
    for path in paths {
        if path.is_absolute()
            && !path.as_os_str().is_empty()
            && shelf.add(Content::File(path.clone()), now())
        {
            if let Some(item) = shelf
                .items
                .iter_mut()
                .find(|item| item.content == Content::File(path.clone()))
            {
                item.folder = path.is_dir();
            }
            added += 1;
        }
    }
    publish(app, &shelf);
    added
}
#[tauri::command]
pub fn shelf_drag_active(app: AppHandle, active: bool) {
    let mut active = active
        && config::lock(app.state::<config::SharedConfig>().inner())
            .shelf
            .files;
    let island = app.state::<state::SharedIsland>();
    state::commit(&app, &island, |machine| {
        active &= machine.current() != state::IslandState::Hidden;
        machine.set_file_drag(active);
    });
    let _ = app.emit("shelf://drag", active);
}

/// Drags one item out to another app. The item is copied, so it stays on the shelf. The drag runs
/// on the main thread, which owns OLE, and this returns when the drag ends.
#[tauri::command]
pub async fn shelf_drag_out(app: AppHandle, id: u64) -> Result<(), String> {
    let payload = {
        let handle = app.state::<ShelfHandle>();
        let shelf = lock(&handle.shelf);
        match shelf
            .items
            .iter()
            .find(|item| item.id == id)
            .map(|item| item.content.clone())
        {
            Some(Content::Text(text)) => shelf_drag::Payload::Text(text),
            Some(Content::File(path)) if path.exists() => shelf_drag::Payload::File(path),
            _ => return Err("Item cannot be dragged".into()),
        }
    };
    let island = app.state::<state::SharedIsland>().inner().clone();
    state::commit(&app, &island, |machine| machine.set_drag_out(true));
    let (sender, receiver) = std::sync::mpsc::channel();
    let started = app.run_on_main_thread(move || {
        let _ = sender.send(shelf_drag::run(payload));
    });
    let outcome = match started {
        Ok(()) => tauri::async_runtime::spawn_blocking(move || receiver.recv())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_else(|| Err("Drag did not start".into())),
        Err(_) => Err("Drag did not start".into()),
    };
    state::commit(&app, &island, |machine| machine.set_drag_out(false));
    outcome
}

fn owned_path(handle: &ShelfHandle, id: u64) -> Result<PathBuf, String> {
    let shelf = lock(&handle.shelf);
    match shelf
        .items
        .iter()
        .find(|item| item.id == id)
        .map(|item| &item.content)
    {
        Some(Content::File(path)) if path.exists() => Ok(path.clone()),
        _ => Err("File is missing".into()),
    }
}
fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
#[tauri::command]
pub fn shelf_open(handle: State<'_, ShelfHandle>, id: u64) -> Result<(), String> {
    let path = wide(&owned_path(&handle, id)?);
    let result = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(path.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize <= 32 {
        Err("Could not open file".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
pub fn shelf_reveal(handle: State<'_, ShelfHandle>, id: u64) -> Result<(), String> {
    let path = owned_path(&handle, id)?;
    // One argument, no command shell: spaces, UNC paths and shortcuts stay literal.
    std::process::Command::new("explorer.exe")
        .arg(format!("/select,\"{}\"", path.display()))
        .spawn()
        .map(|_| ())
        .map_err(|_| "Could not show file".into())
}
#[tauri::command]
pub fn shelf_copy(app: AppHandle, handle: State<'_, ShelfHandle>, id: u64) -> Result<(), String> {
    let mut shelf = lock(&handle.shelf);
    let Some(Content::Text(text)) = shelf
        .items
        .iter()
        .find(|item| item.id == id)
        .map(|item| &item.content)
    else {
        return Err("Text is no longer on the shelf".into());
    };
    let hwnd = app
        .get_webview_window("main")
        .and_then(|window| window.hwnd().ok());
    let Some(hwnd) = hwnd else {
        return Err("Clipboard unavailable".into());
    };
    let sequence =
        write_clipboard(hwnd, text).ok_or_else(|| "Clipboard unavailable".to_string())?;
    handle.copied_sequence.store(sequence, Ordering::Release);
    // Promote here: our own clipboard event is suppressed, including for truncated copies.
    shelf.promote(id, now());
    publish(&app, &shelf);
    Ok(())
}

struct Clipboard;
impl Clipboard {
    fn open(owner: Option<HWND>) -> Option<Self> {
        for _ in 0..4 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Self);
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }
}
impl Drop for Clipboard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

fn read_clipboard() -> Option<(String, u32)> {
    let _clipboard = Clipboard::open(None)?;
    unsafe {
        for (format, exclude_always) in [
            (w!("ExcludeClipboardContentFromMonitorProcessing"), true),
            (w!("CanIncludeInClipboardHistory"), false),
            (w!("CanUploadToCloudClipboard"), false),
        ] {
            let id = RegisterClipboardFormatW(format);
            if id == 0 {
                return None;
            }
            if IsClipboardFormatAvailable(id).is_ok() {
                if exclude_always {
                    return None;
                }
                let data = GetClipboardData(id).ok()?;
                let global = HGLOBAL(data.0);
                if GlobalSize(global) < 4 {
                    return None;
                }
                let ptr = GlobalLock(global);
                if ptr.is_null() {
                    return None;
                }
                let allowed = ptr.cast::<u32>().read_unaligned() != 0;
                let _ = GlobalUnlock(global);
                if !allowed {
                    return None;
                }
            }
        }
        let data = GetClipboardData(UNICODE_TEXT).ok()?;
        let global = HGLOBAL(data.0);
        let size = GlobalSize(global) / 2;
        let ptr = GlobalLock(global).cast::<u16>();
        if ptr.is_null() {
            return None;
        }
        // 20,000 Unicode scalars can occupy at most 40,000 UTF-16 units.
        let units = std::slice::from_raw_parts(ptr, size.min(40_001));
        let end = units.iter().position(|unit| *unit == 0);
        let text = end.and_then(|end| String::from_utf16(&units[..end]).ok());
        let _ = GlobalUnlock(global);
        text.map(|text| (text, GetClipboardSequenceNumber()))
    }
}

fn write_clipboard(hwnd: HWND, text: &str) -> Option<u32> {
    let units: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    let _clipboard = Clipboard::open(Some(hwnd))?;
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, units.len() * 2).ok()?;
        let ptr = GlobalLock(memory).cast::<u16>();
        if ptr.is_null() {
            let _ = GlobalFree(Some(memory));
            return None;
        }
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr, units.len());
        let _ = GlobalUnlock(memory);
        if EmptyClipboard().is_err()
            || SetClipboardData(UNICODE_TEXT, Some(HANDLE(memory.0))).is_err()
        {
            let _ = GlobalFree(Some(memory));
            return None;
        }
        Some(GetClipboardSequenceNumber())
    }
}

struct Listener {
    hwnd: isize,
    thread: JoinHandle<()>,
}
impl Listener {
    fn start(app: AppHandle) -> Option<Self> {
        let (ready, receive) = std::sync::mpsc::sync_channel(1);
        let thread = thread::spawn(move || unsafe {
            let class = w!("NotchyShelfClipboard");
            let instance = GetModuleHandleW(None).ok().map(|module| module.into());
            let wc = WNDCLASSW {
                hInstance: instance.unwrap_or_default(),
                lpfnWndProc: Some(clipboard_proc),
                lpszClassName: class,
                ..Default::default()
            };
            RegisterClassW(&wc);
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class,
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                instance,
                None,
            ) else {
                let _ = ready.send(None);
                return;
            };
            if AddClipboardFormatListener(hwnd).is_err() {
                let _ = DestroyWindow(hwnd);
                let _ = ready.send(None);
                return;
            }
            let _ = ready.send(Some(hwnd.0 as isize));
            let mut message = MSG::default();
            while GetMessageW(&mut message, None, 0, 0).0 > 0 {
                if message.message == WM_CLIPBOARDUPDATE
                    && let Some((text, sequence)) = read_clipboard()
                {
                    let handle = app.state::<ShelfHandle>();
                    let mut shelf = lock(&handle.shelf);
                    if handle.copied_sequence.load(Ordering::Acquire) != sequence
                        && shelf.text(&text, now())
                    {
                        publish(&app, &shelf);
                    }
                }
                DispatchMessageW(&message);
            }
        });
        match receive.recv().ok().flatten() {
            Some(hwnd) => Some(Self { hwnd, thread }),
            None => {
                let _ = thread.join();
                None
            }
        }
    }
    fn stop(self) {
        let _ = unsafe {
            PostMessageW(
                Some(HWND(self.hwnd as *mut _)),
                WM_CLOSE,
                WPARAM(0),
                LPARAM(0),
            )
        };
        let _ = self.thread.join();
    }
}

unsafe extern "system" fn clipboard_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    unsafe {
        if message == WM_CLOSE {
            let _ = RemoveClipboardFormatListener(hwnd);
            let _ = DestroyWindow(hwnd);
            PostQuitMessage(0);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, message, wp, lp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(shelf: &mut Shelf, name: &str, at: u64) -> bool {
        shelf.add(Content::File(PathBuf::from(name)), at)
    }

    #[test]
    fn dedupe_moves_text_and_paths_to_front() {
        let mut shelf = Shelf::default();
        shelf.text("one", 1);
        shelf.text("two", 2);
        shelf.text("one", 3);
        assert_eq!(shelf.items.len(), 2);
        assert_eq!(shelf.items[0].at_ms, 3);
        file(&mut shelf, "a", 4);
        file(&mut shelf, "b", 5);
        file(&mut shelf, "a", 6);
        assert_eq!(shelf.items.len(), 4);
        assert_eq!(shelf.items[0].at_ms, 6);
    }
    #[test]
    fn long_text_identity_includes_the_discarded_suffix() {
        let mut shelf = Shelf::default();
        let prefix = "a".repeat(2000);
        shelf.text(&format!("{prefix}one"), 1);
        shelf.text(&format!("{prefix}two"), 2);
        shelf.text(&format!("{prefix}one"), 3);
        assert_eq!(shelf.items.len(), 2);
        assert_eq!(shelf.items[0].id, 1);
    }

    #[test]
    fn copy_back_promotes_in_its_pin_group_without_adding_an_item() {
        let mut shelf = Shelf::default();
        shelf.text("pinned", 1);
        shelf.text("older", 2);
        shelf.text("newer", 3);
        shelf.pin(1, true);
        shelf.promote(2, 4);
        assert_eq!(shelf.items.iter().map(|item| item.id).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(shelf.items[1].at_ms, 4);
        shelf.promote(1, 5);
        assert_eq!(shelf.items[0].id, 1);
        assert_eq!(shelf.items.len(), 3);
    }
    #[test]
    fn cap_drops_oldest_unpinned_and_all_pinned_refuses() {
        let mut shelf = Shelf::default();
        for i in 0..20 {
            shelf.text(&i.to_string(), i);
        }
        shelf.pin(1, true);
        shelf.text("new", 30);
        assert_eq!(shelf.items.len(), 20);
        assert!(shelf.items.iter().any(|item| item.id == 1));
        assert!(!shelf.items.iter().any(|item| item.id == 2));
        for item in &mut shelf.items {
            item.pinned = true;
        }
        assert!(!shelf.text("refused", 31));
        assert_eq!(shelf.items.len(), 20);
        assert!(shelf.text("new", 32));
        assert_eq!(shelf.items.len(), 20);
    }
    #[test]
    fn pinned_first_then_newest_and_clear_keeps_pinned() {
        let mut shelf = Shelf::default();
        shelf.text("a", 1);
        shelf.text("b", 2);
        shelf.text("c", 3);
        shelf.pin(1, true);
        shelf.pin(2, true);
        assert_eq!(
            shelf.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            [2, 1, 3]
        );
        shelf.text("a", 4);
        assert_eq!(
            shelf.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
        shelf.clear(Kind::Text);
        assert_eq!(shelf.items.len(), 2);
        shelf.pin(1, false);
        assert_eq!(shelf.items[0].id, 2);
    }
    #[test]
    fn clear_removes_only_unpinned_items_of_one_kind() {
        let mut shelf = Shelf::default();
        shelf.text("t1", 1);
        shelf.text("t2", 2);
        file(&mut shelf, "f1", 3);
        file(&mut shelf, "f2", 4);
        shelf.pin(1, true);
        shelf.pin(4, true);
        shelf.clear(Kind::File);
        let kinds: Vec<(u64, Kind)> = shelf
            .items
            .iter()
            .map(|item| (item.id, item.content.kind()))
            .collect();
        assert_eq!(kinds, [(4, Kind::File), (1, Kind::Text), (2, Kind::Text)]);
        shelf.clear(Kind::Text);
        assert_eq!(shelf.items.iter().map(|item| item.id).collect::<Vec<_>>(), [4, 1]);
        shelf.clear(Kind::File);
        assert_eq!(shelf.items.iter().map(|item| item.id).collect::<Vec<_>>(), [4, 1]);
    }
    #[test]
    fn kind_parses_the_wire_names_the_page_sends() {
        let text: Kind = serde_json::from_str("\"text\"").unwrap();
        let file: Kind = serde_json::from_str("\"file\"").unwrap();
        assert!(text == Kind::Text && file == Kind::File);
        assert!(serde_json::from_str::<Kind>("\"Files\"").is_err());
    }
    #[test]
    fn text_limits_and_preview_keep_unicode_and_line_breaks() {
        let mut shelf = Shelf::default();
        assert!(!shelf.text(" \n\t", 0));
        assert!(!shelf.text("", 0));
        assert!(!shelf.text(&"a".repeat(20_001), 0));
        assert!(shelf.text(&"😀".repeat(20_000), 1));
        let Content::Text(text) = &shelf.items[0].content else {
            panic!()
        };
        assert_eq!(text.chars().count(), 2000);
        assert_eq!(
            views(&shelf)[0].preview.as_ref().unwrap().chars().count(),
            240
        );
        shelf.text("a\nb", 2);
        assert_eq!(views(&shelf)[0].preview.as_deref(), Some("a\nb"));
    }
    #[test]
    fn clipboard_off_drops_even_pinned_text_but_keeps_files() {
        let mut shelf = Shelf::default();
        shelf.text("a", 1);
        shelf.pin(1, true);
        file(&mut shelf, "b", 2);
        shelf.disable_clipboard();
        assert_eq!(shelf.items.len(), 1);
        assert!(matches!(shelf.items[0].content, Content::File(_)));
    }
}
