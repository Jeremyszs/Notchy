//! Tray icon and the settings window it opens. Left-click opens Settings, and the menu offers
//! Settings and Quit. Quit is the only way to exit, since closing Settings only hides it.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::Color;
use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder, WindowEvent};

const SETTINGS_LABEL: &str = "settings";

pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Notchy", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&settings, &quit])?;

    let mut tray = TrayIconBuilder::new()
        .tooltip("Notchy")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "settings" => open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                open_settings(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Shows the settings window, creating it on first use. Building a webview window from inside a
/// menu or tray handler can deadlock on Windows, so the work goes to the async runtime.
pub fn open_settings(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }

        let built = WebviewWindowBuilder::new(
            &app,
            SETTINGS_LABEL,
            WebviewUrl::App("settings.html".into()),
        )
        .title("Notchy Settings")
        .inner_size(720.0, 560.0)
        .min_inner_size(560.0, 420.0)
        .center()
        .resizable(true)
        .skip_taskbar(false)
        .theme(Some(Theme::Dark))
        .background_color(Color(0, 0, 0, 255))
        .build();

        match built {
            Ok(window) => {
                // Closing hides the window, so reopening is instant and the app keeps running.
                let handle = window.clone();
                window.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = handle.hide();
                    }
                });
            }
            Err(error) => eprintln!("notchy: could not open settings: {error}"),
        }
    });
}
