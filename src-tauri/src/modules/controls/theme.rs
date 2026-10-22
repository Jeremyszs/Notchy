//! Dark mode, through the per-user Personalize values that Settings writes. The value is 0
//! for dark and 1 for light.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_DWORD, RRF_RT_REG_DWORD, RegGetValueW, RegSetKeyValueW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
};
use windows::core::{PCWSTR, w};

use super::{Control, Kind, Reading, error_text, flag, is_on};

/// The light-theme value for `dark`. Windows stores 0 for dark and 1 for light.
fn light_value(dark: bool) -> u32 {
    if dark { 0 } else { 1 }
}

fn is_dark(light: u32) -> bool {
    light == 0
}

fn read_light(name: PCWSTR) -> Result<u32, String> {
    let mut data = 0u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut c_void),
            Some(&mut size as *mut u32),
        )
    }
    .ok()
    .map_err(|error| error_text(&error))?;
    Ok(data)
}

fn write_light(name: PCWSTR, light: u32) -> Result<(), String> {
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            name,
            REG_DWORD.0,
            Some(&light as *const u32 as *const c_void),
            size_of::<u32>() as u32,
        )
    }
    .ok()
    .map_err(|error| error_text(&error))
}

/// Tells open windows that the colour set changed. The timeout and abort-if-hung flag keep one
/// stuck window from holding up the toggle.
fn broadcast_theme_change() {
    let area = w!("ImmersiveColorSet");
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(area.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            1000,
            None,
        )
    };
}

pub(super) struct DarkMode;

impl Control for DarkMode {
    fn id(&self) -> &'static str {
        "darkmode"
    }

    fn label(&self) -> &'static str {
        "Dark mode"
    }

    fn kind(&self) -> Kind {
        Kind::Toggle
    }

    fn read(&self) -> Result<Reading, String> {
        let light = read_light(w!("AppsUseLightTheme"))?;
        Ok(Reading {
            value: flag(is_dark(light)),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let light = light_value(is_on(value));
        write_light(w!("AppsUseLightTheme"), light)?;
        write_light(w!("SystemUsesLightTheme"), light)?;
        broadcast_theme_change();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_is_stored_as_zero() {
        assert_eq!(light_value(true), 0);
        assert_eq!(light_value(false), 1);
    }

    #[test]
    fn registry_value_maps_back_to_dark_mode() {
        assert!(is_dark(0));
        assert!(!is_dark(1));
        assert!(!is_dark(7));
        assert!(is_dark(light_value(true)));
        assert!(!is_dark(light_value(false)));
    }
}
