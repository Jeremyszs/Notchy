//! Opt-in global shortcuts. Registration stays in Rust; the windows only read the status.

use std::sync::{Mutex, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use super::{Module, ModuleContext, media, timer};
use crate::config::{self, SharedConfig};

const STATUS_EVENT: &str = "hotkeys://status";

#[derive(Debug, Clone, Serialize)]
pub struct BindingStatus {
    pub action: String,
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct Hub {
    registered: Vec<Shortcut>,
    status: Vec<BindingStatus>,
}

pub struct HotkeysModule;

impl Module for HotkeysModule {
    fn name(&self) -> &'static str {
        "hotkeys"
    }

    fn start(&self, ctx: ModuleContext) {
        ctx.app.manage(Mutex::new(Hub::default()));
        reapply(&ctx.app);
    }
}

pub fn reapply(app: &AppHandle) {
    // Keep registration and status reads on the main thread: the plugin dispatches there and waits.
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || apply(&handle));
}

fn apply(app: &AppHandle) {
    let Some(hub) = app.try_state::<Mutex<Hub>>() else {
        return;
    };
    let mut hub = hub.lock().unwrap_or_else(PoisonError::into_inner);
    let settings = config::lock(&app.state::<SharedConfig>()).hotkeys.clone();
    let shortcuts = app.global_shortcut();
    let mut retained = Vec::new();
    let mut removal_error = None;
    for shortcut in hub.registered.drain(..) {
        if let Err(error) = shortcuts.unregister(shortcut) {
            retained.push(shortcut);
            removal_error = Some(error.to_string());
        }
    }
    hub.registered = retained;
    hub.status.clear();
    for (action, binding) in settings.bindings() {
        let mut error = removal_error.clone();
        if error.is_none() && settings.enabled && !binding.is_empty() {
            let result = binding
                .parse::<Shortcut>()
                .map_err(|error| error.to_string())
                .and_then(|shortcut| {
                    shortcuts
                        .on_shortcut(shortcut, move |app, _, event| {
                            if event.state != ShortcutState::Pressed {
                                return;
                            }
                            match action {
                                "playPause" => media::send_control(app, media::Control::PlayPause),
                                "nextTrack" => media::send_control(app, media::Control::Next),
                                "previousTrack" => {
                                    media::send_control(app, media::Control::Previous)
                                }
                                "timerToggle" => timer::toggle(app),
                                _ => {}
                            }
                        })
                        .map_err(|error| error.to_string())?;
                    hub.registered.push(shortcut);
                    Ok(())
                });
            error = result.err();
        }
        hub.status.push(BindingStatus {
            action: action.into(),
            ok: error.is_none(),
            error,
        });
    }
    let _ = app.emit(STATUS_EVENT, &hub.status);
}

pub fn shutdown(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}

#[tauri::command]
pub fn hotkeys_status(app: AppHandle) -> Vec<BindingStatus> {
    app.try_state::<Mutex<Hub>>()
        .map(|hub| {
            hub.lock()
                .unwrap_or_else(PoisonError::into_inner)
                .status
                .clone()
        })
        .unwrap_or_default()
}
