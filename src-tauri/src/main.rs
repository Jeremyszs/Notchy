// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod modules;
mod single_instance;
mod state;
mod tray;
mod window;

use std::sync::{Arc, Mutex};

use modules::timer::TimerHandle;
use state::{IslandStateMachine, SharedIsland};
use tauri::Manager;

fn main() {
    // A second copy exits here, silently, before it creates a window or a tray icon. The guard
    // lives until main returns, so the name stays claimed for the whole run.
    let Some(_instance) = single_instance::claim(single_instance::MUTEX_NAME) else {
        return;
    };
    // Before Tauri or any module touches COM. See `init_process_security`.
    modules::controls::init_process_security();
    tauri::Builder::default()
        .manage::<SharedIsland>(Arc::new(Mutex::new(IslandStateMachine::default())))
        .manage(modules::audio::AudioController::default())
        .manage(Arc::new(TimerHandle::default()))
        .setup(|app| {
            let path = config::file_in(&app.path().app_config_dir()?);
            let settings = config::load(&path);
            config::sync_launch_at_login(settings.launch_at_login);
            app.manage::<config::SharedConfig>(Arc::new(Mutex::new(settings)));
            app.manage(config::ConfigFile(path));

            window::init(app.handle())?;
            modules::start_all(app.handle());
            tray::init(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            modules::media::media_play_pause,
            modules::media::media_next,
            modules::media::media_previous,
            modules::media::media_snapshot,
            modules::lyrics::lyrics_snapshot,
            modules::audio::audio_set_active,
            state::island_set_wide,
            modules::controls::controls_list,
            modules::controls::controls_set,
            modules::controls::controls_press,
            modules::controls::network::wifi_networks,
            modules::controls::network::wifi_connect,
            modules::controls::network::bluetooth_devices,
            modules::controls::network::open_settings,
            modules::notifications::notifications_snapshot,
            modules::battery::battery_snapshot,
            modules::timer::timer_snapshot,
            modules::timer::timer_select,
            modules::timer::timer_start,
            modules::timer::timer_pause,
            modules::timer::timer_resume,
            modules::timer::timer_reset,
            modules::timer::timer_add_ms,
            modules::timer::timer_lap,
            modules::timer::timer_dismiss,
            config::config_get,
            config::config_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Notchy");
}
