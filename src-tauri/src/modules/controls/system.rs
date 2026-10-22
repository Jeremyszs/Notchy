//! Lock, screenshot and keep-awake.
//!
//! The execution state belongs to the thread that set it, so keep-awake parks a dedicated
//! thread for as long as it is on.

use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::thread;

use windows::Win32::System::Power::{
    ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState,
};
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use super::{Control, Kind, Reading, error_text, flag, is_on};

/// The stop signal for the thread that holds the system awake. Dropping it ends the hold.
static AWAKE: Mutex<Option<Sender<()>>> = Mutex::new(None);

fn awake_held() -> bool {
    AWAKE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
}

fn hold_awake(stop: Receiver<()>) {
    unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED) };
    // Nothing is ever sent. recv returns once the sender is dropped.
    let _ = stop.recv();
    unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
}

pub(super) struct Lock;

impl Control for Lock {
    fn id(&self) -> &'static str {
        "lock"
    }

    fn label(&self) -> &'static str {
        "Lock"
    }

    fn kind(&self) -> Kind {
        Kind::Button
    }

    fn read(&self) -> Result<Reading, String> {
        Ok(Reading {
            value: 0.0,
            detail: None,
        })
    }

    fn press(&self) -> Result<(), String> {
        unsafe { LockWorkStation() }.map_err(|error| error_text(&error))
    }
}

pub(super) struct Screenshot;

impl Control for Screenshot {
    fn id(&self) -> &'static str {
        "screenshot"
    }

    fn label(&self) -> &'static str {
        "Screenshot"
    }

    fn kind(&self) -> Kind {
        Kind::Button
    }

    fn read(&self) -> Result<Reading, String> {
        Ok(Reading {
            value: 0.0,
            detail: None,
        })
    }

    fn press(&self) -> Result<(), String> {
        let launched = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("ms-screenclip:"),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };
        // ShellExecute reports success as a value above 32.
        if launched.0 as usize > 32 {
            Ok(())
        } else {
            Err("Could not open the Snipping Tool".to_owned())
        }
    }
}

pub(super) struct Awake;

impl Control for Awake {
    fn id(&self) -> &'static str {
        "awake"
    }

    fn label(&self) -> &'static str {
        "Keep awake"
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        Ok(Reading {
            value: flag(awake_held()),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let mut held = AWAKE.lock().unwrap_or_else(PoisonError::into_inner);
        if !is_on(value) {
            // Dropping the sender wakes the holder, which releases the hold and exits.
            *held = None;
            return Ok(());
        }
        if held.is_none() {
            let (stop, parked) = mpsc::channel();
            thread::Builder::new()
                .name("notchy-awake".to_owned())
                .spawn(move || hold_awake(parked))
                .map_err(|error| error.to_string())?;
            *held = Some(stop);
        }
        Ok(())
    }
}
