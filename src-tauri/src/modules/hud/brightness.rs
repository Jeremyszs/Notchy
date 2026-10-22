//! Built-in panel brightness changes. WMI raises an event each time the panel's driver changes the
//! level, whether a brightness key or the Settings app did it, so nothing polls. Each event reads
//! the level again and reports it when it differs from the last one seen.

use std::sync::mpsc::Sender;
use std::time::Instant;

use super::{HudUpdate, Kind, brightness_step, own_brightness_write_recent};
use crate::modules::controls::{ComGuard, brightness_percent, watch_brightness};

/// Thread body. Runs for the life of the app.
pub(super) fn run(updates: Sender<HudUpdate>) {
    // WMI needs COM on this thread. Without it there is nothing to watch.
    let Some(_com) = ComGuard::init() else {
        return;
    };
    let mut last = brightness_percent();
    watch_brightness(|| {
        let Some(level) = brightness_percent() else {
            return;
        };
        let own = own_brightness_write_recent(Instant::now());
        if let Some(level) = brightness_step(&mut last, level, own) {
            let _ = updates.send(HudUpdate {
                kind: Kind::Brightness,
                level,
                muted: false,
            });
        }
    });
}
