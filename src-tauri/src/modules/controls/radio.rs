//! Wi-Fi and Bluetooth radios through Windows.Devices.Radios, plus airplane mode built on them.
//!
//! Windows has no public airplane-mode API. Airplane mode here turns every radio off and
//! remembers which ones were on, so turning it off restores that set.

use std::sync::{Mutex, PoisonError};

use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};

use super::{Control, Kind, Reading, error_text, flag, is_on, wait};

/// Radios that were on when airplane mode was last turned on. None means it was never turned
/// on in this run, so turning it off turns every radio back on.
static AIRPLANE_SAVED: Mutex<Option<Vec<RadioKind>>> = Mutex::new(None);

/// Every radio on the machine. Access is requested first, since WinRT refuses the calls without it.
pub(super) fn radios() -> Result<Vec<Radio>, String> {
    let access: RadioAccessStatus = wait(Radio::RequestAccessAsync())?;
    if access != RadioAccessStatus::Allowed {
        return Err("Radio access was denied".to_owned());
    }
    let list = wait(Radio::GetRadiosAsync())?;
    let count = list.Size().map_err(|error| error_text(&error))?;
    (0..count)
        .map(|index| list.GetAt(index).map_err(|error| error_text(&error)))
        .collect()
}

pub(super) fn kind_of(radio: &Radio) -> RadioKind {
    radio.Kind().unwrap_or(RadioKind::Other)
}

pub(super) fn is_on_state(radio: &Radio) -> bool {
    radio.State().is_ok_and(|state| state == RadioState::On)
}

/// Turns one radio on or off. Windows refuses the change when the radio is disabled.
fn set_radio(radio: &Radio, on: bool) -> Result<(), String> {
    let state = if on { RadioState::On } else { RadioState::Off };
    let status: RadioAccessStatus = wait(radio.SetStateAsync(state))?;
    if status == RadioAccessStatus::Allowed {
        Ok(())
    } else {
        Err("Windows did not allow the change".to_owned())
    }
}

/// Airplane mode reads as on when there are radios and every one is off.
fn airplane_on(states: &[bool]) -> bool {
    !states.is_empty() && states.iter().all(|on| !on)
}

/// The kinds to turn on when airplane mode ends. A remembered set limits the restore to those
/// kinds. With nothing remembered, every radio comes back on.
fn restore_targets<T: Copy + PartialEq>(all: &[T], remembered: Option<&[T]>) -> Vec<T> {
    match remembered {
        Some(saved) => all
            .iter()
            .copied()
            .filter(|kind| saved.contains(kind))
            .collect(),
        None => all.to_vec(),
    }
}

/// A Wi-Fi or Bluetooth switch. `radio` picks the first radio of that kind.
pub(super) struct RadioToggle {
    id: &'static str,
    label: &'static str,
    radio: RadioKind,
    missing: &'static str,
}

pub(super) static WIFI: RadioToggle = RadioToggle {
    id: "wifi",
    label: "Wi-Fi",
    radio: RadioKind::WiFi,
    missing: "No Wi-Fi adapter",
};

pub(super) static BLUETOOTH: RadioToggle = RadioToggle {
    id: "bluetooth",
    label: "Bluetooth",
    radio: RadioKind::Bluetooth,
    missing: "No Bluetooth adapter",
};

impl RadioToggle {
    fn first(&self) -> Result<Radio, String> {
        radios()?
            .into_iter()
            .find(|radio| kind_of(radio) == self.radio)
            .ok_or_else(|| self.missing.to_owned())
    }
}

impl Control for RadioToggle {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> &'static str {
        self.label
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        let radio = self.first()?;
        Ok(Reading {
            value: flag(is_on_state(&radio)),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let radio = self.first()?;
        set_radio(&radio, is_on(value))
    }
}

pub(super) struct Airplane;

impl Control for Airplane {
    fn id(&self) -> &'static str {
        "airplane"
    }

    fn label(&self) -> &'static str {
        "Airplane mode"
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        let states: Vec<bool> = radios()?.iter().map(is_on_state).collect();
        if states.is_empty() {
            return Err("No wireless radios".to_owned());
        }
        Ok(Reading {
            value: flag(airplane_on(&states)),
            detail: Some("Turns all radios off".to_owned()),
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let radios = radios()?;
        let kinds: Vec<RadioKind> = radios.iter().map(kind_of).collect();
        let states: Vec<bool> = radios.iter().map(is_on_state).collect();
        let mut saved = AIRPLANE_SAVED
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if is_on(value) {
            if airplane_on(&states) {
                return Ok(());
            }
            *saved = Some(
                kinds
                    .iter()
                    .zip(&states)
                    .filter(|(_, on)| **on)
                    .map(|(kind, _)| *kind)
                    .collect(),
            );
            // Best effort: a radio Windows refuses to change shows up as still on in the next read.
            for radio in &radios {
                let _ = set_radio(radio, false);
            }
        } else {
            let targets = restore_targets(&kinds, saved.as_deref());
            *saved = None;
            for (radio, kind) in radios.iter().zip(&kinds) {
                let _ = set_radio(radio, targets.contains(kind));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airplane_needs_radios_and_all_of_them_off() {
        assert!(airplane_on(&[false, false]));
        assert!(!airplane_on(&[false, true]));
        assert!(!airplane_on(&[]));
    }

    #[test]
    fn restore_brings_back_only_remembered_radios() {
        assert_eq!(restore_targets(&[1, 2, 3], Some(&[2])), vec![2]);
        assert_eq!(restore_targets(&[1, 2, 3], Some(&[9])), Vec::<i32>::new());
    }

    #[test]
    fn restore_with_nothing_remembered_turns_everything_on() {
        assert_eq!(restore_targets(&[1, 2, 3], None), vec![1, 2, 3]);
    }

    #[test]
    fn restore_of_an_empty_remembered_set_turns_nothing_on() {
        assert_eq!(restore_targets(&[1, 2, 3], Some(&[])), Vec::<i32>::new());
    }
}
