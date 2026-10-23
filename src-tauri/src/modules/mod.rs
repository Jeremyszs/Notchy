//! Feature modules. Each one runs its own background work and reports to the rest of the
//! app through `state::commit` and Tauri events. Modules never touch the overlay window.
//!
//! To add one: create `modules/<name>.rs` with a unit struct implementing `Module`, add
//! it to the array in `start_all`, and register its commands in the `invoke_handler` in
//! main.rs.

pub mod audio;
pub mod context;
pub mod controls;
pub mod hud;
pub mod lyrics;
pub mod media;
pub mod timer;

use tauri::{AppHandle, Manager};

use crate::state::SharedIsland;

#[derive(Clone)]
pub struct ModuleContext {
    pub app: AppHandle,
    pub island: SharedIsland,
}

pub trait Module {
    fn name(&self) -> &'static str;

    /// Runs once during setup. Must return quickly, so spawn any long-lived work.
    fn start(&self, ctx: ModuleContext);
}

pub fn start_all(app: &AppHandle) {
    let ctx = ModuleContext {
        app: app.clone(),
        island: app.state::<SharedIsland>().inner().clone(),
    };
    let modules: [&dyn Module; 5] = [
        &media::MediaModule,
        &lyrics::LyricsModule,
        &context::ContextModule,
        &hud::HudModule,
        &timer::TimerModule,
    ];
    for module in modules {
        if cfg!(debug_assertions) {
            eprintln!("notchy: starting module {}", module.name());
        }
        module.start(ctx.clone());
    }
}
