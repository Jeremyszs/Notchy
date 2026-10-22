//! Wi-Fi networks, saved Wi-Fi profiles and paired Bluetooth devices, for the Controls sub-views.
//!
//! Each list is read when its sub-view opens, never on a timer. Wi-Fi scans through
//! Windows.Devices.WiFi and reads the connected network from the connection profiles. Saved
//! profiles come from `netsh`, since WinRT has no call for them. Bluetooth is read-only: Windows
//! has no public API to connect or disconnect a device, so the page links to Settings instead.

use std::os::windows::process::CommandExt;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Devices::Bluetooth::{
    BluetoothConnectionStatus, BluetoothDevice, BluetoothMajorClass,
};
use windows::Devices::Enumeration::DeviceInformation;
use windows::Devices::Radios::RadioKind;
use windows::Devices::WiFi::{WiFiAccessStatus, WiFiAdapter, WiFiAvailableNetwork};
use windows::Networking::Connectivity::{
    NetworkAuthenticationType, NetworkConnectivityLevel, NetworkEncryptionType, NetworkInformation,
};
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{PCWSTR, w};

use super::radio;
use super::{blocking, error_text, wait};

/// The longest SSID 802.11 allows, in bytes.
const SSID_MAX_BYTES: usize = 32;
/// How long a Wi-Fi scan may run before the adapter's last report is used instead.
const SCAN_TIMEOUT: Duration = Duration::from_secs(4);
const SCAN_POLL: Duration = Duration::from_millis(50);
// Windows.Foundation.AsyncStatus values. The type is not re-exported by the windows crate.
const ASYNC_STARTED: i32 = 0;
const ASYNC_ERROR: i32 = 3;
/// Keeps netsh from opening a console window in this GUI app.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Shown when scanning is blocked. Windows only lets desktop apps scan while location is allowed.
const LOCATION_HINT: &str = "Allow location access for desktop apps to list networks";
const NO_WIFI: &str = "No Wi-Fi adapter";
const WIFI_OFF: &str = "Wi-Fi is off";
const NO_BLUETOOTH: &str = "No Bluetooth adapter";
const BLUETOOTH_OFF: &str = "Bluetooth is off";
const NEEDS_PASSWORD: &str = "Needs a password: open Windows Wi-Fi settings";

/// One Wi-Fi network, one row per SSID. `signal` is 0..=100 percent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal: u8,
    pub secured: bool,
    pub connected: bool,
    /// Has a saved profile, so it can be joined without a password.
    pub known: bool,
}

/// One paired Bluetooth device. `kind` is "audio", "input", "phone" or "other".
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BtDevice {
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub kind: &'static str,
}

/// A network as the scan reported it, before it is merged by SSID.
#[derive(Debug, Clone, PartialEq)]
struct Scanned {
    ssid: String,
    signal: u8,
    secured: bool,
}

/// Turns scan results into rows. Hidden networks (empty SSID) are dropped, and each SSID keeps its
/// strongest reading. The connected network comes first, then by signal, then by name.
fn arrange(scanned: Vec<Scanned>, connected: Option<&str>, known: &[String]) -> Vec<WifiNetwork> {
    let mut best: Vec<Scanned> = Vec::new();
    for network in scanned
        .into_iter()
        .filter(|network| !network.ssid.is_empty())
    {
        match best.iter().position(|kept| kept.ssid == network.ssid) {
            Some(index) => {
                if network.signal > best[index].signal {
                    best[index] = network;
                }
            }
            None => best.push(network),
        }
    }

    let mut list: Vec<WifiNetwork> = best
        .into_iter()
        .map(|network| WifiNetwork {
            connected: connected == Some(network.ssid.as_str()),
            known: known.contains(&network.ssid),
            ssid: network.ssid,
            signal: network.signal,
            secured: network.secured,
        })
        .collect();
    list.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    list
}

/// Maps a received power in dBm to 0..=100: -100 dBm or weaker is 0, -50 dBm or stronger is 100.
fn signal_percent(dbm: f64) -> u8 {
    if dbm.is_nan() {
        return 0;
    }
    ((dbm + 100.0) * 2.0).clamp(0.0, 100.0).round() as u8
}

/// The saved profile names from `netsh wlan show profiles`. Lines look like
/// `    All User Profile     : Home`. The name is everything after the first colon, so a name
/// that itself contains a colon is kept whole.
fn parse_profiles(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let (key, name) = line.split_once(':')?;
            (key.trim() == "All User Profile").then(|| name.trim_start().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// Checks a name before it reaches netsh: 1 to 32 bytes, with no control characters.
fn validate_ssid(ssid: &str) -> Result<(), String> {
    if ssid.is_empty() || ssid.len() > SSID_MAX_BYTES || ssid.chars().any(char::is_control) {
        return Err("Invalid network name".to_owned());
    }
    Ok(())
}

/// Only saved networks join from here. Anything else needs a password, which this app never asks for.
fn connect_allowed(ssid: &str, known: &[String]) -> Result<(), String> {
    validate_ssid(ssid)?;
    if known.iter().any(|name| name == ssid) {
        Ok(())
    } else {
        Err(NEEDS_PASSWORD.to_owned())
    }
}

/// The Settings page each allowed name opens. Anything else is refused.
fn settings_uri(page: &str) -> Option<&'static str> {
    match page {
        "wifi" => Some("ms-settings:network-wifi"),
        "bluetooth" => Some("ms-settings:bluetooth"),
        "location" => Some("ms-settings:privacy-location"),
        _ => None,
    }
}

/// Bluetooth devices sort connected first, then by name without regard to case.
fn sort_devices(devices: &mut [BtDevice]) {
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// The coarse kind the page draws for a Bluetooth major device class.
fn bt_kind(major: BluetoothMajorClass) -> &'static str {
    match major {
        BluetoothMajorClass::AudioVideo => "audio",
        BluetoothMajorClass::Peripheral => "input",
        BluetoothMajorClass::Phone => "phone",
        _ => "other",
    }
}

/// Runs netsh with each argument passed separately, never through a shell, and no console window.
fn run_netsh(args: &[&str]) -> Result<Output, String> {
    Command::new("netsh")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| error.to_string())
}

/// The saved Wi-Fi profiles. A netsh failure reads as none, so every network shows as unknown.
fn saved_profiles() -> Vec<String> {
    run_netsh(&["wlan", "show", "profiles"])
        .map(|output| parse_profiles(&String::from_utf8_lossy(&output.stdout)))
        .unwrap_or_default()
}

/// The SSID of the Wi-Fi network this machine is on, from its connection profiles.
fn connected_ssid() -> Option<String> {
    let profiles = NetworkInformation::GetConnectionProfiles().ok()?;
    let count = profiles.Size().ok()?;
    (0..count).find_map(|index| {
        let profile = profiles.GetAt(index).ok()?;
        let linked = profile.GetNetworkConnectivityLevel().ok()? != NetworkConnectivityLevel::None;
        if !linked || !profile.IsWlanConnectionProfile().ok()? {
            return None;
        }
        let ssid = profile
            .WlanConnectionProfileDetails()
            .ok()?
            .GetConnectedSsid()
            .ok()?;
        Some(ssid.to_string())
    })
}

/// Fails with a clear message when the radio of `kind` is missing or switched off.
fn radio_on(kind: RadioKind, missing: &str, off: &str) -> Result<(), String> {
    let found = radio::radios()?
        .into_iter()
        .find(|candidate| radio::kind_of(candidate) == kind)
        .ok_or_else(|| missing.to_owned())?;
    if radio::is_on_state(&found) {
        Ok(())
    } else {
        Err(off.to_owned())
    }
}

/// Scan refusals come back as access denied when location is off for desktop apps.
fn scan_error(error: &windows::core::Error) -> String {
    if error.code() == E_ACCESSDENIED {
        LOCATION_HINT.to_owned()
    } else {
        error_text(error)
    }
}

fn first_adapter() -> Result<WiFiAdapter, String> {
    let adapters = wait(WiFiAdapter::FindAllAdaptersAsync())?;
    let count = adapters.Size().map_err(|error| error_text(&error))?;
    if count == 0 {
        return Err(NO_WIFI.to_owned());
    }
    adapters.GetAt(0).map_err(|error| error_text(&error))
}

/// Starts a scan and waits up to SCAN_TIMEOUT. A scan that is still running then is cancelled,
/// and the adapter's last report is used instead.
fn scan(adapter: &WiFiAdapter) -> Result<(), String> {
    let operation = adapter.ScanAsync().map_err(|error| scan_error(&error))?;
    let deadline = Instant::now() + SCAN_TIMEOUT;
    loop {
        match operation.Status().map(|status| status.0) {
            Ok(ASYNC_STARTED) => {
                if Instant::now() >= deadline {
                    let _ = operation.Cancel();
                    return Ok(());
                }
                thread::sleep(SCAN_POLL);
            }
            Ok(ASYNC_ERROR) => {
                return operation.GetResults().map_err(|error| scan_error(&error));
            }
            _ => return Ok(()),
        }
    }
}

/// Whether a network asks for a password. A network whose security cannot be read counts as secured.
fn is_secured(network: &WiFiAvailableNetwork) -> bool {
    let Ok(settings) = network.SecuritySettings() else {
        return true;
    };
    let auth = settings
        .NetworkAuthenticationType()
        .unwrap_or(NetworkAuthenticationType::Unknown);
    let encryption = settings
        .NetworkEncryptionType()
        .unwrap_or(NetworkEncryptionType::Unknown);
    let open = (auth == NetworkAuthenticationType::None
        || auth == NetworkAuthenticationType::Open80211)
        && encryption == NetworkEncryptionType::None;
    !open
}

fn read_network(network: &WiFiAvailableNetwork) -> Option<Scanned> {
    Some(Scanned {
        ssid: network.Ssid().ok()?.to_string(),
        signal: signal_percent(network.NetworkRssiInDecibelMilliwatts().ok()?),
        secured: is_secured(network),
    })
}

fn bt_device(info: &DeviceInformation) -> Option<BtDevice> {
    let id = info.Id().ok()?;
    let name = info.Name().ok()?.to_string();
    let device: BluetoothDevice = wait(BluetoothDevice::FromIdAsync(&id)).ok()?;
    let connected = device.ConnectionStatus().ok()? == BluetoothConnectionStatus::Connected;
    let kind = device
        .ClassOfDevice()
        .ok()
        .and_then(|class| class.MajorClass().ok())
        .map_or("other", bt_kind);
    Some(BtDevice {
        id: id.to_string(),
        name: if name.is_empty() {
            "Unnamed device".to_owned()
        } else {
            name
        },
        connected,
        kind,
    })
}

/// Visible Wi-Fi networks, one per SSID, connected first. Each scan is time-boxed.
#[tauri::command]
pub async fn wifi_networks() -> Result<Vec<WifiNetwork>, String> {
    blocking(|| {
        radio_on(RadioKind::WiFi, NO_WIFI, WIFI_OFF)?;
        let access = wait(WiFiAdapter::RequestAccessAsync())?;
        if access != WiFiAccessStatus::Allowed {
            return Err(LOCATION_HINT.to_owned());
        }
        let adapter = first_adapter()?;
        scan(&adapter)?;

        let report = adapter
            .NetworkReport()
            .map_err(|error| error_text(&error))?;
        let available = report
            .AvailableNetworks()
            .map_err(|error| error_text(&error))?;
        let count = available.Size().map_err(|error| error_text(&error))?;
        let scanned: Vec<Scanned> = (0..count)
            .filter_map(|index| available.GetAt(index).ok())
            .filter_map(|network| read_network(&network))
            .collect();

        let connected = connected_ssid();
        let known = saved_profiles();
        Ok(arrange(scanned, connected.as_deref(), &known))
    })
    .await
}

/// Joins a saved network. Unknown networks are refused, and the page sends the user to Settings.
#[tauri::command]
pub async fn wifi_connect(ssid: String) -> Result<(), String> {
    blocking(move || {
        connect_allowed(&ssid, &saved_profiles())?;
        let name = format!("name={ssid}");
        let output = run_netsh(&["wlan", "connect", name.as_str()])?;
        if output.status.success() {
            Ok(())
        } else {
            Err("Could not connect to that network".to_owned())
        }
    })
    .await
}

/// Paired Bluetooth devices, connected first. Read-only.
#[tauri::command]
pub async fn bluetooth_devices() -> Result<Vec<BtDevice>, String> {
    blocking(|| {
        radio_on(RadioKind::Bluetooth, NO_BLUETOOTH, BLUETOOTH_OFF)?;
        let selector = BluetoothDevice::GetDeviceSelectorFromPairingState(true)
            .map_err(|error| error_text(&error))?;
        let found = wait(DeviceInformation::FindAllAsyncAqsFilter(&selector))?;
        let mut devices: Vec<BtDevice> = found
            .into_iter()
            .filter_map(|info| bt_device(&info))
            .collect();
        sort_devices(&mut devices);
        Ok(devices)
    })
    .await
}

fn open_uri(uri: &str) -> Result<(), String> {
    let wide: Vec<u16> = uri.encode_utf16().chain(std::iter::once(0)).collect();
    let launched = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute reports success as a value above 32.
    if launched.0 as usize > 32 {
        Ok(())
    } else {
        Err("Could not open Windows settings".to_owned())
    }
}

/// Opens one of the allowed Settings pages: "wifi", "bluetooth" or "location".
#[tauri::command]
pub async fn open_settings(page: String) -> Result<(), String> {
    let uri = settings_uri(&page).ok_or_else(|| "Unknown settings page".to_owned())?;
    blocking(move || open_uri(uri)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scanned(ssid: &str, signal: u8, secured: bool) -> Scanned {
        Scanned {
            ssid: ssid.to_owned(),
            signal,
            secured,
        }
    }

    #[test]
    fn signal_maps_dbm_to_percent() {
        assert_eq!(signal_percent(-30.0), 100);
        assert_eq!(signal_percent(-50.0), 100);
        assert_eq!(signal_percent(-75.0), 50);
        assert_eq!(signal_percent(-100.0), 0);
        assert_eq!(signal_percent(-120.0), 0);
        assert_eq!(signal_percent(f64::NAN), 0);
    }

    #[test]
    fn profiles_parse_the_netsh_line_format() {
        let output = "\r\nProfiles on interface Wi-Fi:\r\n\r\nGroup policy profiles (read only)\r\n---------------------------------\r\n    <None>\r\n\r\n    User profiles\r\n    -------------\r\n    All User Profile     : Home Wi-Fi\r\n    All User Profile     : Work: Floor 2\r\n    All User Profile     :\r\n";
        assert_eq!(
            parse_profiles(output),
            vec!["Home Wi-Fi".to_owned(), "Work: Floor 2".to_owned()]
        );
    }

    #[test]
    fn profiles_ignore_other_lines() {
        assert!(parse_profiles("    User profiles\n    <None>\n").is_empty());
        assert!(parse_profiles("").is_empty());
    }

    #[test]
    fn arrange_drops_hidden_and_keeps_the_strongest_per_ssid() {
        let list = arrange(
            vec![
                scanned("", 90, false),
                scanned("Cafe", 40, true),
                scanned("Cafe", 70, true),
                scanned("Home", 55, true),
            ],
            None,
            &[],
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].ssid, "Cafe");
        assert_eq!(list[0].signal, 70);
        assert_eq!(list[1].ssid, "Home");
    }

    #[test]
    fn arrange_puts_the_connected_network_first_then_by_signal() {
        let list = arrange(
            vec![
                scanned("Alpha", 90, true),
                scanned("Beta", 30, true),
                scanned("Gamma", 60, false),
            ],
            Some("Beta"),
            &[],
        );
        let order: Vec<&str> = list.iter().map(|n| n.ssid.as_str()).collect();
        assert_eq!(order, ["Beta", "Alpha", "Gamma"]);
        assert!(list[0].connected);
        assert!(!list[1].connected);
    }

    #[test]
    fn arrange_marks_saved_networks_known() {
        let list = arrange(
            vec![scanned("Home", 60, true), scanned("Guest", 60, true)],
            None,
            &["Home".to_owned()],
        );
        let home = list.iter().find(|n| n.ssid == "Home").unwrap();
        let guest = list.iter().find(|n| n.ssid == "Guest").unwrap();
        assert!(home.known);
        assert!(!guest.known);
    }

    #[test]
    fn ssids_must_be_short_and_printable() {
        assert!(validate_ssid("Home Wi-Fi").is_ok());
        assert!(validate_ssid(&"a".repeat(32)).is_ok());
        assert!(validate_ssid("").is_err());
        assert!(validate_ssid(&"a".repeat(33)).is_err());
        assert!(validate_ssid("bad\nname").is_err());
        assert!(validate_ssid("bad\u{7}name").is_err());
    }

    #[test]
    fn only_saved_networks_connect() {
        let known = ["Home".to_owned()];
        assert!(connect_allowed("Home", &known).is_ok());
        assert_eq!(
            connect_allowed("Cafe", &known),
            Err(NEEDS_PASSWORD.to_owned())
        );
        assert!(connect_allowed("bad\nname", &known).is_err());
    }

    #[test]
    fn settings_pages_are_an_allow_list() {
        assert_eq!(settings_uri("wifi"), Some("ms-settings:network-wifi"));
        assert_eq!(settings_uri("bluetooth"), Some("ms-settings:bluetooth"));
        assert_eq!(
            settings_uri("location"),
            Some("ms-settings:privacy-location")
        );
        assert_eq!(settings_uri("network-proxy"), None);
        assert_eq!(settings_uri(""), None);
    }

    #[test]
    fn devices_sort_connected_first_then_by_name() {
        let device = |name: &str, connected: bool| BtDevice {
            id: name.to_owned(),
            name: name.to_owned(),
            connected,
            kind: "other",
        };
        let mut devices = vec![
            device("zeta", false),
            device("Buds", true),
            device("alpha", false),
        ];
        sort_devices(&mut devices);
        let order: Vec<&str> = devices.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(order, ["Buds", "alpha", "zeta"]);
    }
}
