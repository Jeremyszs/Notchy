//! Screen brightness. The built-in panel answers the video miniport's brightness IOCTLs on
//! `\\.\LCD`, or WMI in `root\WMI`. Other displays answer DDC/CI through Dxva2 on the primary
//! monitor. The order is LCD, then WMI, then DDC/CI.

use std::ffi::c_void;
use std::mem::{ManuallyDrop, size_of};
use std::thread;
use std::time::Duration;

use crate::modules::hud::note_own_brightness_write;

/// How long to wait before resubscribing to brightness events after a failure.
const WATCH_RETRY: Duration = Duration::from_secs(5);

use windows::Win32::Devices::Display::{
    DISPLAY_BRIGHTNESS, DISPLAYPOLICY_AC, DISPLAYPOLICY_DC, DestroyPhysicalMonitors,
    GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS, IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
    IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, POINT};
use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoInitializeSecurity, EOAC_NONE, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Variant::{
    VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_UI1, VT_UI4,
};
use windows::Win32::System::Wmi::{
    CIM_UINT8, CIM_UINT32, IEnumWbemClassObject, IWbemClassObject, IWbemContext, IWbemLocator,
    IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_FLAG_RETURN_WBEM_COMPLETE, WBEM_INFINITE, WbemLocator,
};
use windows::core::{BSTR, IUnknown, w};

use super::{Control, Kind, Reading, error_text, percent};

const UNSUPPORTED: &str = "Not supported by this display";

/// Both power sources, so the level holds on battery and on AC.
const DISPLAYPOLICY_BOTH: u8 = (DISPLAYPOLICY_AC | DISPLAYPOLICY_DC) as u8;

pub(super) struct Brightness;

impl Control for Brightness {
    fn id(&self) -> &'static str {
        "brightness"
    }

    fn label(&self) -> &'static str {
        "Brightness"
    }

    fn kind(&self) -> Kind {
        Kind::Slider
    }

    fn read(&self) -> Result<Reading, String> {
        let level = read_percent().ok_or_else(|| UNSUPPORTED.to_owned())?;
        Ok(Reading {
            value: f64::from(level),
            detail: None,
        })
    }

    fn write(&self, value: f64) -> Result<(), String> {
        let level = percent(value) as u8;
        // Marked before the write, so the change it causes is not taken for a key press.
        note_own_brightness_write();
        lcd_write(level)
            .or_else(|| wmi_write(level).ok())
            .or_else(|| ddc_write(level))
            .ok_or_else(|| UNSUPPORTED.to_owned())
    }
}

/// Current brightness as a percent, from the first source that answers: the panel's LCD device,
/// then WMI, then DDC/CI.
pub(crate) fn read_percent() -> Option<u8> {
    lcd_read()
        .or_else(|| wmi_read().ok())
        .or_else(ddc_read)
        .and_then(|level| u8::try_from(level).ok())
}

/// Calls `changed` each time the built-in panel reports a brightness change. Subscribes through
/// WMI's `WmiMonitorBrightnessEvent`, which the panel's driver raises on a key press or a change in
/// the Settings app, so nothing polls. Blocks for the life of the app and resubscribes after a
/// failure. The caller's thread must hold COM.
pub(crate) fn watch_changes(mut changed: impl FnMut()) {
    loop {
        if let Err(error) = wmi_watch(&mut changed)
            && cfg!(debug_assertions)
        {
            eprintln!("notchy: brightness events unavailable: {error}");
        }
        thread::sleep(WATCH_RETRY);
    }
}

/// One WMI subscription. Returns only when the subscription fails. A notification query takes
/// RETURN_IMMEDIATELY with FORWARD_ONLY. FORWARD_ONLY alone is refused with WBEM_E_INVALID_PARAMETER.
fn wmi_watch(changed: &mut impl FnMut()) -> Result<(), String> {
    let services = wmi_services()?;
    let events = unsafe {
        services.ExecNotificationQuery(
            &BSTR::from("WQL"),
            &BSTR::from("SELECT * FROM WmiMonitorBrightnessEvent"),
            WBEM_FLAG_RETURN_IMMEDIATELY | WBEM_FLAG_FORWARD_ONLY,
            None::<&IWbemContext>,
        )
    }
    .map_err(|error| format!("ExecNotificationQuery {:#010x}", error.code().0))?;
    loop {
        let mut slot: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0;
        unsafe { events.Next(WBEM_INFINITE, &mut slot, &mut returned) }
            .ok()
            .map_err(|error| format!("Next {:#010x}", error.code().0))?;
        if returned > 0 {
            changed();
        }
    }
}

/// An open `\\.\LCD` handle, closed on drop.
struct LcdHandle(HANDLE);

impl Drop for LcdHandle {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The laptop panel device. Opened read and write, since the set IOCTL needs write access.
fn lcd_open() -> Option<LcdHandle> {
    unsafe {
        CreateFileW(
            w!("\\\\.\\LCD"),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
    }
    .ok()
    .map(LcdHandle)
}

/// The brightness levels the panel accepts, as percents.
fn lcd_levels(lcd: &LcdHandle) -> Option<Vec<u8>> {
    let mut buffer = [0u8; 256];
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            lcd.0,
            IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS,
            None,
            0,
            Some(buffer.as_mut_ptr().cast::<c_void>()),
            buffer.len() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    let count = (returned as usize).min(buffer.len());
    let mut levels = buffer[..count].to_vec();
    levels.sort_unstable();
    levels.dedup();
    (!levels.is_empty()).then_some(levels)
}

/// The panel's current level on AC power, as a percent.
fn lcd_current(lcd: &LcdHandle) -> Option<u8> {
    let mut brightness = DISPLAY_BRIGHTNESS::default();
    let mut returned = 0;
    unsafe {
        DeviceIoControl(
            lcd.0,
            IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS,
            None,
            0,
            Some(&mut brightness as *mut DISPLAY_BRIGHTNESS as *mut c_void),
            size_of::<DISPLAY_BRIGHTNESS>() as u32,
            Some(&mut returned),
            None,
        )
    }
    .ok()?;
    Some(brightness.ucACBrightness)
}

fn lcd_read() -> Option<u32> {
    let lcd = lcd_open()?;
    lcd_current(&lcd).map(u32::from)
}

/// Sets the panel to the supported level nearest `level`, on both power sources.
fn lcd_write(level: u8) -> Option<()> {
    let lcd = lcd_open()?;
    let target = nearest_level(&lcd_levels(&lcd)?, level)?;
    let brightness = DISPLAY_BRIGHTNESS {
        ucDisplayPolicy: DISPLAYPOLICY_BOTH,
        ucACBrightness: target,
        ucDCBrightness: target,
    };
    unsafe {
        DeviceIoControl(
            lcd.0,
            IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS,
            Some(&brightness as *const DISPLAY_BRIGHTNESS as *const c_void),
            size_of::<DISPLAY_BRIGHTNESS>() as u32,
            None,
            0,
            None,
            None,
        )
    }
    .ok()
}

/// The supported level closest to `target`. Ties go to the lower level.
fn nearest_level(levels: &[u8], target: u8) -> Option<u8> {
    levels
        .iter()
        .copied()
        .min_by_key(|level| (level.abs_diff(target), *level))
}

/// Sets the process's COM security for WMI. Call it on the main thread before any other COM use.
/// Windows applies default security at the first COM call, and after that `CoInitializeSecurity`
/// is refused. Event subscriptions then fail with WBEM_E_ACCESS_DENIED, so the brightness HUD
/// would never see a change. The main thread takes its apartment first, which is what Tauri expects
/// anyway, and a repeat call simply fails.
pub(crate) fn init_process_security() {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let _ = unsafe {
        CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        )
    };
}

/// A connection to the `root\WMI` namespace. The caller's thread must hold COM.
fn wmi_services() -> Result<IWbemServices, String> {
    unsafe {
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None::<&IUnknown>, CLSCTX_INPROC_SERVER)
                .map_err(|error| error_text(&error))?;
        locator
            .ConnectServer(
                &BSTR::from("ROOT\\WMI"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None::<&IWbemContext>,
            )
            .map_err(|error| error_text(&error))
    }
}

/// The first row of a query result.
fn first_row(rows: &IEnumWbemClassObject) -> Result<IWbemClassObject, String> {
    let mut slot: [Option<IWbemClassObject>; 1] = [None];
    let mut returned = 0;
    unsafe { rows.Next(WBEM_INFINITE, &mut slot, &mut returned) }
        .ok()
        .map_err(|error| error_text(&error))?;
    if returned == 0 {
        return Err("WMI returned no rows".to_owned());
    }
    slot[0]
        .take()
        .ok_or_else(|| "WMI returned no rows".to_owned())
}

fn wmi_query(services: &IWbemServices, query: &str) -> Result<IEnumWbemClassObject, String> {
    unsafe {
        services
            .ExecQuery(
                &BSTR::from("WQL"),
                &BSTR::from(query),
                WBEM_FLAG_FORWARD_ONLY,
                None::<&IWbemContext>,
            )
            .map_err(|error| error_text(&error))
    }
}

/// Current brightness of the built-in panel, as a percent.
fn wmi_read() -> Result<u32, String> {
    let services = wmi_services()?;
    let rows = wmi_query(
        &services,
        "SELECT CurrentBrightness FROM WmiMonitorBrightness",
    )?;
    let row = first_row(&rows)?;
    let mut value = VARIANT::default();
    unsafe { row.Get(w!("CurrentBrightness"), 0, &mut value, None, None) }
        .map_err(|error| error_text(&error))?;
    variant_uint(&value).ok_or_else(|| "Unexpected brightness value".to_owned())
}

/// Sets the built-in panel to `level` percent through the WmiSetBrightness method.
fn wmi_write(level: u8) -> Result<(), String> {
    let services = wmi_services()?;
    let rows = wmi_query(&services, "SELECT * FROM WmiMonitorBrightnessMethods")?;
    // The method lives on one instance per panel, so the call goes to that instance's path.
    let panel = first_row(&rows)?;
    let mut path = VARIANT::default();
    unsafe { panel.Get(w!("__PATH"), 0, &mut path, None, None) }
        .map_err(|error| error_text(&error))?;
    let path = BSTR::try_from(&path).map_err(|error| error_text(&error))?;

    unsafe {
        let mut class = None;
        services
            .GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                WBEM_FLAG_RETURN_WBEM_COMPLETE,
                None::<&IWbemContext>,
                Some(&mut class as *mut _),
                None,
            )
            .map_err(|error| error_text(&error))?;
        let class: IWbemClassObject = class.ok_or_else(|| "WMI class not found".to_owned())?;

        let mut in_params = None;
        let mut out_params = None;
        class
            .GetMethod(w!("WmiSetBrightness"), 0, &mut in_params, &mut out_params)
            .map_err(|error| error_text(&error))?;
        let in_params = in_params.ok_or_else(|| "WmiSetBrightness has no inputs".to_owned())?;
        let call = in_params
            .SpawnInstance(0)
            .map_err(|error| error_text(&error))?;

        call.Put(w!("Timeout"), 0, &long_variant(1), CIM_UINT32.0)
            .map_err(|error| error_text(&error))?;
        call.Put(w!("Brightness"), 0, &byte_variant(level), CIM_UINT8.0)
            .map_err(|error| error_text(&error))?;

        services
            .ExecMethod(
                &path,
                &BSTR::from("WmiSetBrightness"),
                WBEM_FLAG_RETURN_WBEM_COMPLETE,
                None::<&IWbemContext>,
                &call,
                None,
                None,
            )
            .map_err(|error| error_text(&error))
    }
}

/// WMI hands byte-sized properties back as VT_I4, so both integer forms are accepted.
fn variant_uint(value: &VARIANT) -> Option<u32> {
    i32::try_from(value)
        .ok()
        .and_then(|number| u32::try_from(number).ok())
        .or_else(|| u32::try_from(value).ok())
}

/// A VT_UI1 value, the type the byte-sized Brightness parameter expects.
fn byte_variant(value: u8) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI1,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { bVal: value },
            }),
        },
    }
}

/// A VT_UI4 value, the type of the uint32 Timeout parameter.
fn long_variant(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
}

/// Runs `action` on the first physical monitor behind the primary display. Every handle is
/// released again before returning.
fn with_ddc_monitor<T>(action: impl FnOnce(HANDLE) -> Option<T>) -> Option<T> {
    unsafe {
        let display = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut count = 0;
        GetNumberOfPhysicalMonitorsFromHMONITOR(display, &mut count).ok()?;
        if count == 0 {
            return None;
        }
        let mut monitors: Vec<PHYSICAL_MONITOR> = (0..count).map(|_| std::mem::zeroed()).collect();
        GetPhysicalMonitorsFromHMONITOR(display, &mut monitors).ok()?;
        let result = monitors
            .first()
            .and_then(|monitor| action(monitor.hPhysicalMonitor));
        let _ = DestroyPhysicalMonitors(&monitors);
        result
    }
}

fn ddc_read() -> Option<u32> {
    with_ddc_monitor(|handle| {
        let (mut min, mut current, mut max) = (0, 0, 0);
        let ok = unsafe { GetMonitorBrightness(handle, &mut min, &mut current, &mut max) };
        if ok == 0 {
            return None;
        }
        percent_of(current, min, max)
    })
}

fn ddc_write(level: u8) -> Option<()> {
    with_ddc_monitor(|handle| {
        let (mut min, mut current, mut max) = (0, 0, 0);
        let ok = unsafe { GetMonitorBrightness(handle, &mut min, &mut current, &mut max) };
        if ok == 0 {
            return None;
        }
        let target = level_for(level, min, max);
        (unsafe { SetMonitorBrightness(handle, target) } != 0).then_some(())
    })
}

/// Where `current` sits in the `min..=max` range, as a rounded percent. None for an empty range.
fn percent_of(current: u32, min: u32, max: u32) -> Option<u32> {
    let span = max.checked_sub(min).filter(|span| *span > 0)?;
    let offset = current.saturating_sub(min).min(span);
    Some((f64::from(offset) * 100.0 / f64::from(span)).round() as u32)
}

/// The DDC/CI level for a percent of the `min..=max` range.
fn level_for(percent: u8, min: u32, max: u32) -> u32 {
    let span = max.saturating_sub(min);
    min + (f64::from(span) * f64::from(percent.min(100)) / 100.0).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_of_maps_the_range_onto_0_to_100() {
        assert_eq!(percent_of(0, 0, 100), Some(0));
        assert_eq!(percent_of(50, 0, 100), Some(50));
        assert_eq!(percent_of(75, 25, 125), Some(50));
        assert_eq!(percent_of(255, 0, 255), Some(100));
    }

    #[test]
    fn percent_of_clamps_and_rejects_empty_ranges() {
        assert_eq!(percent_of(300, 0, 100), Some(100));
        assert_eq!(percent_of(5, 10, 10), None);
        assert_eq!(percent_of(5, 20, 10), None);
    }

    #[test]
    fn level_for_inverts_percent_of() {
        assert_eq!(level_for(0, 0, 100), 0);
        assert_eq!(level_for(50, 0, 100), 50);
        assert_eq!(level_for(100, 25, 125), 125);
        assert_eq!(percent_of(level_for(40, 25, 125), 25, 125), Some(40));
    }

    #[test]
    fn variant_uint_accepts_wmi_integer_forms() {
        assert_eq!(variant_uint(&long_variant(80)), Some(80));
        assert_eq!(variant_uint(&long_variant(0)), Some(0));
    }

    #[test]
    fn nearest_level_picks_the_closest_supported_step() {
        let levels = [0, 25, 50, 75, 100];
        assert_eq!(nearest_level(&levels, 0), Some(0));
        assert_eq!(nearest_level(&levels, 37), Some(25));
        assert_eq!(nearest_level(&levels, 38), Some(50));
        assert_eq!(nearest_level(&levels, 100), Some(100));
        assert_eq!(nearest_level(&[], 40), None);
    }

    #[test]
    fn lcd_ioctl_codes_match_the_video_driver_values() {
        assert_eq!(IOCTL_VIDEO_QUERY_SUPPORTED_BRIGHTNESS, 0x0023_0494);
        assert_eq!(IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS, 0x0023_0498);
        assert_eq!(IOCTL_VIDEO_SET_DISPLAY_BRIGHTNESS, 0x0023_049C);
    }
}
