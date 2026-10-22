//! Quick settings for the notch: volume, mute, brightness, radios, lock, screenshot and a few
//! app-level switches. Each setting is a `Control` in a submodule. The registry below lists
//! them, and the commands look them up by id.
//!
//! Nothing here polls. The frontend asks for `controls_list` when the notch opens. OS work runs
//! on the blocking pool, and each job sets up COM for its own thread.

mod audio;
mod display;
pub(crate) mod network;
mod radio;
mod system;
mod theme;

pub(crate) use display::{
    init_process_security, read_percent as brightness_percent, watch_changes as watch_brightness,
};

use std::future::IntoFuture;

use serde::Serialize;
use tauri::async_runtime::block_on;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::Result as WinResult;

/// How the notch draws a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Toggle,
    Slider,
    Button,
}

/// The frontend contract. Mirrors the ControlInfo type the notch UI reads.
///
/// `value` is 0..=100 for sliders, 0 or 1 for toggles, and 0 for buttons. `supported` is false
/// when the control cannot work on this machine, and `detail` then says why.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlInfo {
    pub id: String,
    pub label: String,
    pub kind: Kind,
    pub supported: bool,
    pub value: f64,
    pub detail: Option<String>,
}

/// What a control reports when read. `detail` is a short status line for the notch.
struct Reading {
    value: f64,
    detail: Option<String>,
}

/// One quick setting. Read and write run on the blocking pool, inside a `ComGuard`.
trait Control: Send + Sync {
    fn id(&self) -> &'static str;
    fn label(&self) -> &'static str;
    fn kind(&self) -> Kind;

    /// The current state. An error means the control is unavailable, and its text is shown.
    fn read(&self) -> Result<Reading, String>;

    /// Sets a slider (0..=100) or a toggle (0 or 1).
    fn write(&self, _value: f64) -> Result<(), String> {
        Err("This control has no value to set".to_owned())
    }

    /// Runs a button.
    fn press(&self) -> Result<(), String> {
        Err("This control is not a button".to_owned())
    }
}

/// Every control, in the order the notch lists them.
static CONTROLS: &[&dyn Control] = &[
    &audio::Volume,
    &audio::Mute,
    &audio::Mic,
    &display::Brightness,
    &radio::WIFI,
    &radio::BLUETOOTH,
    &radio::Airplane,
    &system::Lock,
    &system::Screenshot,
    &system::Awake,
    &theme::DarkMode,
];

/// CoInitializeEx must be paired with CoUninitialize on the same thread. Only the success path
/// (including S_FALSE) increments the count, so only that path gets undone.
pub(crate) struct ComGuard;

impl ComGuard {
    pub(crate) fn init() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

/// Runs `work` on the blocking pool with COM set up for that thread.
async fn blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || {
        let _com = ComGuard::init();
        work()
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Blocks until a WinRT async operation completes. Only called from the blocking pool.
fn wait<O, T>(op: WinResult<O>) -> Result<T, String>
where
    O: IntoFuture<Output = WinResult<T>>,
{
    let future = op.map_err(|error| error_text(&error))?.into_future();
    block_on(future).map_err(|error| error_text(&error))
}

fn error_text(error: &windows::core::Error) -> String {
    error.message().trim().to_owned()
}

fn find(id: &str) -> Result<&'static dyn Control, String> {
    CONTROLS
        .iter()
        .copied()
        .find(|control| control.id() == id)
        .ok_or_else(|| format!("Unknown control: {id}"))
}

/// The kind of a registered control. None when the registry does not know the id.
pub fn kind_of(id: &str) -> Option<Kind> {
    find(id).ok().map(|control| control.kind())
}

/// Builds the frontend view of one control. A failed read makes it unsupported.
fn describe(control: &dyn Control) -> ControlInfo {
    let (supported, value, detail) = match control.read() {
        Ok(reading) => (true, reading.value, reading.detail),
        Err(reason) => (false, 0.0, Some(reason)),
    };
    ControlInfo {
        id: control.id().to_owned(),
        label: control.label().to_owned(),
        kind: control.kind(),
        supported,
        value,
        detail,
    }
}

/// Slider values are whole percents in 0..=100. NaN counts as 0.
fn percent(value: f64) -> f64 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 100.0).round()
    }
}

/// Toggles are on from one half up. NaN is off.
fn is_on(value: f64) -> bool {
    value >= 0.5
}

fn flag(on: bool) -> f64 {
    if on { 1.0 } else { 0.0 }
}

#[tauri::command]
pub async fn controls_list() -> Vec<ControlInfo> {
    blocking(|| Ok(CONTROLS.iter().map(|control| describe(*control)).collect()))
        .await
        .unwrap_or_default()
}

#[tauri::command]
pub async fn controls_set(id: String, value: f64) -> Result<ControlInfo, String> {
    blocking(move || {
        let control = find(&id)?;
        control.write(value)?;
        Ok(describe(control))
    })
    .await
}

#[tauri::command]
pub async fn controls_press(id: String) -> Result<(), String> {
    blocking(move || find(&id)?.press()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_clamps_and_rounds() {
        assert_eq!(percent(150.0), 100.0);
        assert_eq!(percent(-3.0), 0.0);
        assert_eq!(percent(49.4), 49.0);
        assert_eq!(percent(49.5), 50.0);
        assert_eq!(percent(f64::NAN), 0.0);
    }

    #[test]
    fn toggle_threshold_is_one_half() {
        assert!(is_on(1.0));
        assert!(is_on(0.5));
        assert!(!is_on(0.49));
        assert!(!is_on(0.0));
        assert!(!is_on(f64::NAN));
    }

    #[test]
    fn control_ids_are_unique_and_findable() {
        let mut ids: Vec<&str> = CONTROLS.iter().map(|control| control.id()).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total);
        for id in ids {
            assert!(find(id).is_ok());
        }
        assert!(find("nope").is_err());
    }

    #[test]
    fn unsupported_read_reports_its_reason() {
        struct Broken;
        impl Control for Broken {
            fn id(&self) -> &'static str {
                "broken"
            }
            fn label(&self) -> &'static str {
                "Broken"
            }
            fn kind(&self) -> Kind {
                Kind::Slider
            }
            fn read(&self) -> Result<Reading, String> {
                Err("No device".to_owned())
            }
        }

        let info = describe(&Broken);
        assert!(!info.supported);
        assert_eq!(info.value, 0.0);
        assert_eq!(info.detail.as_deref(), Some("No device"));
    }

    #[test]
    fn kinds_serialize_lowercase() {
        assert_eq!(serde_json::to_string(&Kind::Toggle).unwrap(), "\"toggle\"");
        assert_eq!(serde_json::to_string(&Kind::Slider).unwrap(), "\"slider\"");
        assert_eq!(serde_json::to_string(&Kind::Button).unwrap(), "\"button\"");
    }
}
