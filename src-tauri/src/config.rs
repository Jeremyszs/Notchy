//! User settings, kept in `config.json` in the app config directory. The file is read once at
//! startup and written whole on every change. A missing or corrupt file gives the defaults.
//!
//! `config_set` is the only writer. It sanitizes, saves atomically, applies what changed (the
//! launch-at-login entry and lyrics), and emits `config://update` to every window.

use std::fs::{self, File};
use std::io::{self, Write};
use std::iter;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Once, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::w;

use crate::modules::calendar;
use crate::modules::controls::{self, Kind};
use crate::modules::lyrics;
use crate::modules::notifications;

const FILE_NAME: &str = "config.json";
const UPDATE_EVENT: &str = "config://update";
/// Page ids the notch knows how to show, in their default order.
pub const PAGES: [&str; 5] = ["media", "controls", "timer", "stats", "calendar"];
/// Pomodoro lengths in whole minutes: the bounds the settings and the file are held to.
pub const FOCUS_MINUTES: (u32, u32, u32) = (25, 1, 120);
pub const SHORT_BREAK_MINUTES: (u32, u32, u32) = (5, 1, 60);
pub const LONG_BREAK_MINUTES: (u32, u32, u32) = (15, 1, 60);
const DEFAULT_CONTROLS: [&str; 8] = [
    "volume",
    "brightness",
    "wifi",
    "bluetooth",
    "airplane",
    "mute",
    "awake",
    "lock",
];
/// The tile row fits seven buttons and toggles. Sliders sit above the row and are not counted.
pub const MAX_TILES: usize = 7;
/// Font ids the notch and settings know how to draw. Mirrors src/lib/font.ts.
pub const FONTS: [&str; 7] = [
    "sf",
    "inter",
    "manrope",
    "dm-sans",
    "space-grotesk",
    "playfair",
    "system",
];
const DEFAULT_FONT: &str = "inter";

/// The frontend contract. Mirrors the Config type in src/stores/config.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Shown pages, in order.
    pub pages: Vec<String>,
    /// Shown quick controls, in order.
    pub controls: Vec<String>,
    pub lyrics: bool,
    pub confirm_lock: bool,
    pub launch_at_login: bool,
    /// The font id from FONTS. Unknown ids are sanitized to the default.
    pub font: String,
    /// Briefly show the volume and brightness HUD when either changes.
    pub hud: bool,
    /// Play the system beep once when a timer or a pomodoro phase runs out.
    pub timer_sound: bool,
    /// Show the time (HH:MM, dim) in the idle pill.
    pub idle_clock: bool,
    /// Pomodoro focus length, in minutes. Held to FOCUS_MINUTES.
    pub focus_minutes: u32,
    /// Pomodoro short break length, in minutes. Held to SHORT_BREAK_MINUTES.
    pub short_break_minutes: u32,
    /// Pomodoro long break length, in minutes. Held to LONG_BREAK_MINUTES.
    pub long_break_minutes: u32,
    /// Other apps' toast notifications, shown in the island.
    pub notifications: Notifications,
    /// Charger and battery-level pills in the island.
    pub battery: Battery,
    /// The system stats page.
    pub stats: Stats,
    /// Calendar reminders and the Calendar page.
    pub calendar: Calendar,
}

/// The stats page's settings. Mirrors `stats` in src/stores/config.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Stats {
    /// Show the Stats page and sample the system while it is on screen. Off, the page is hidden and
    /// nothing is read.
    pub enabled: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Self { enabled: true }
    }
}


/// The calendar's reminder settings. Mirrors `calendar` in src/stores/config.ts. The events
/// themselves are in `events.json`, not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Calendar {
    /// Reminders for the calendar's events, and the Calendar page. On by default.
    pub enabled: bool,
    /// Minutes before an event to remind at. One of LEAD_CHOICES.
    pub lead_minutes: u32,
    /// The hour, 0 to 23, an all-day event is announced at on its day.
    pub all_day_hour: u8,
}

/// The lead times the Settings window offers, in minutes.
pub const LEAD_CHOICES: [u32; 5] = [5, 10, 15, 30, 60];
const DEFAULT_LEAD_MINUTES: u32 = 10;
const DEFAULT_ALL_DAY_HOUR: u8 = 9;

impl Default for Calendar {
    fn default() -> Self {
        Self {
            enabled: true,
            lead_minutes: DEFAULT_LEAD_MINUTES,
            all_day_hour: DEFAULT_ALL_DAY_HOUR,
        }
    }
}

impl Calendar {
    /// A lead time outside LEAD_CHOICES falls back to the default, and an hour past 23 is held to 23.
    fn sanitize(self) -> Self {
        Self {
            lead_minutes: if LEAD_CHOICES.contains(&self.lead_minutes) {
                self.lead_minutes
            } else {
                DEFAULT_LEAD_MINUTES
            },
            all_day_hour: self.all_day_hour.min(23),
            ..self
        }
    }
}

/// The battery feature's settings. Mirrors `battery` in src/stores/config.ts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Battery {
    /// Battery alerts. On by default. Off, nothing is shown, though the status line in Settings
    /// still reads the battery.
    pub enabled: bool,
    /// Show a pill when the charger is plugged in or out, and when the battery is full.
    pub charge_alerts: bool,
    /// Show a warning at 20% and at 10% while on battery.
    pub low_battery_alerts: bool,
}

impl Default for Battery {
    fn default() -> Self {
        Self {
            enabled: true,
            charge_alerts: true,
            low_battery_alerts: true,
        }
    }
}

/// The notification feature's settings. Mirrors `notifications` in src/stores/config.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notifications {
    /// Show other apps' notifications. On by default, and only read while on.
    pub enabled: bool,
    /// App ids (AppUserModelIds) whose notifications are ignored. Every other app is shown.
    pub denied_apps: Vec<String>,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            enabled: true,
            denied_apps: Vec::new(),
        }
    }
}

impl Notifications {
    /// Trims app ids, drops empty ones and repeats. Pure, like `Config::sanitize`.
    fn sanitize(self) -> Self {
        let trimmed = self
            .denied_apps
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
            .collect();
        Self {
            denied_apps: keep_unique(trimmed, |_| true),
            ..self
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pages: PAGES.map(String::from).to_vec(),
            controls: DEFAULT_CONTROLS.map(String::from).to_vec(),
            lyrics: true,
            confirm_lock: true,
            launch_at_login: false,
            font: DEFAULT_FONT.to_string(),
            hud: true,
            timer_sound: true,
            idle_clock: true,
            focus_minutes: FOCUS_MINUTES.0,
            short_break_minutes: SHORT_BREAK_MINUTES.0,
            long_break_minutes: LONG_BREAK_MINUTES.0,
            notifications: Notifications::default(),
            battery: Battery::default(),
            stats: Stats::default(),
            calendar: Calendar::default(),
        }
    }
}

/// Keeps a minute setting inside its bounds. Out-of-range values are clamped, not reset.
fn clamp_minutes((_, min, max): (u32, u32, u32), value: u32) -> u32 {
    value.clamp(min, max)
}

impl Config {
    /// Keeps only what the notch can show: known page and control ids, each once, in the order
    /// given. At least one page remains, and tiles stop at MAX_TILES. An unknown font falls back
    /// to the default. Pure, so it runs on load and on every save.
    pub fn sanitize(self) -> Self {
        let font = if FONTS.contains(&self.font.as_str()) {
            self.font
        } else {
            DEFAULT_FONT.to_string()
        };

        let mut pages = keep_unique(self.pages, |id| PAGES.contains(&id));
        if pages.is_empty() {
            pages = Self::default().pages;
        }

        let mut tiles = 0;
        let controls = keep_unique(self.controls, |id| match controls::kind_of(id) {
            Some(Kind::Slider) => true,
            Some(_) => {
                tiles += 1;
                tiles <= MAX_TILES
            }
            None => false,
        });

        Self {
            pages,
            controls,
            font,
            focus_minutes: clamp_minutes(FOCUS_MINUTES, self.focus_minutes),
            short_break_minutes: clamp_minutes(SHORT_BREAK_MINUTES, self.short_break_minutes),
            long_break_minutes: clamp_minutes(LONG_BREAK_MINUTES, self.long_break_minutes),
            notifications: self.notifications.sanitize(),
            calendar: self.calendar.sanitize(),
            ..self
        }
    }
}

/// Keeps the ids that `keep` accepts, in order, skipping repeats. A repeat is checked before
/// `keep` runs, so it never uses up a tile slot.
fn keep_unique(ids: Vec<String>, mut keep: impl FnMut(&str) -> bool) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for id in ids {
        if !kept.contains(&id) && keep(&id) {
            kept.push(id);
        }
    }
    kept
}

/// The settings in memory. Commands and modules read it, and only `config_set` replaces it.
pub type SharedConfig = Arc<Mutex<Config>>;

/// Where `config.json` lives. Managed next to the shared settings.
pub struct ConfigFile(pub PathBuf);

pub fn lock(shared: &SharedConfig) -> MutexGuard<'_, Config> {
    // Each change swaps in a whole value, so a poisoned lock still holds a valid config.
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Whether lyrics are on. Defaults to on when no config is managed yet.
pub fn lyrics_enabled(app: &AppHandle) -> bool {
    app.try_state::<SharedConfig>()
        .is_none_or(|config| lock(&config).lyrics)
}

/// Whether the volume and brightness HUD shows. Defaults to on when no config is managed yet.
pub fn hud_enabled(app: &AppHandle) -> bool {
    app.try_state::<SharedConfig>()
        .is_none_or(|config| lock(&config).hud)
}

/// Whether other apps' notifications are shown. Defaults to on when no config is managed yet.
pub fn notifications_enabled(app: &AppHandle) -> bool {
    app.try_state::<SharedConfig>()
        .is_none_or(|config| lock(&config).notifications.enabled)
}

/// The battery alert settings. The defaults when no config is managed yet.
pub fn battery(app: &AppHandle) -> Battery {
    app.try_state::<SharedConfig>()
        .map(|config| lock(&config).battery)
        .unwrap_or_default()
}

/// Whether the stats page is on. Defaults to on when no config is managed yet.
pub fn stats_enabled(app: &AppHandle) -> bool {
    app.try_state::<SharedConfig>()
        .is_none_or(|config| lock(&config).stats.enabled)
}

/// The calendar reminder settings. The defaults when no config is managed yet.
pub fn calendar(app: &AppHandle) -> Calendar {
    app.try_state::<SharedConfig>()
        .map(|config| lock(&config).calendar)
        .unwrap_or_default()
}

/// The app ids whose notifications are ignored. Empty when no config is managed yet.
pub fn denied_apps(app: &AppHandle) -> Vec<String> {
    app.try_state::<SharedConfig>()
        .map(|config| lock(&config).notifications.denied_apps.clone())
        .unwrap_or_default()
}

/// The settings file inside the app config directory.
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Reads the settings file. A missing or unreadable file, and invalid JSON, give the defaults.
/// A partial file keeps the fields it has. A leading UTF-8 byte order mark is skipped, since
/// Windows PowerShell writes one and a parser would otherwise reject the whole file.
pub fn load(path: &Path) -> Config {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Config>(strip_bom(&text)).ok())
        .map_or_else(Config::default, Config::sanitize)
}

/// The text without a leading byte order mark (U+FEFF), if it has one.
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// Writes through a temp file and a rename, so a crash never leaves half a file behind.
pub fn save(path: &Path, config: &Config) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(config).map_err(io::Error::other)?;
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp)?;
    file.write_all(&json)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp, path)
}

/// What the Run entry should become. Decided by `plan_run_entry`, which is pure so the rules are
/// tested without touching the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunEntry {
    /// Point the entry at this exe.
    Write(PathBuf),
    /// Delete the entry.
    Remove,
    /// Leave the entry as it is, for this reason.
    Skip(&'static str),
}

/// Whether `exe` is a build output that must never be registered: its parent directory is named
/// `debug`, or it sits under a `target\debug` path. Case-insensitive, like Windows paths.
pub fn is_dev_exe(exe: &Path) -> bool {
    let parts: Vec<String> = exe
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    let parent_is_debug = parts.len() >= 2 && parts[parts.len() - 2] == "debug";
    parent_is_debug
        || parts
            .windows(2)
            .any(|pair| pair[0] == "target" && pair[1] == "debug")
}

/// Decides the Run entry for a setting. Debug builds never touch it (a debug exe needs the Vite
/// dev server and opens a console, so it must not start at sign-in), and a release build only
/// registers an exe that `is_dev_exe` accepts. Turning the setting off removes the entry.
pub fn plan_run_entry(enabled: bool, debug_build: bool, exe: Option<&Path>) -> RunEntry {
    if debug_build {
        return RunEntry::Skip("debug build: launch at login is not changed");
    }
    if !enabled {
        return RunEntry::Remove;
    }
    match exe {
        None => RunEntry::Skip("could not find the running exe"),
        Some(exe) if is_dev_exe(exe) => RunEntry::Skip("refusing to register a debug build exe"),
        Some(exe) => RunEntry::Write(exe.to_path_buf()),
    }
}

/// Writes or removes the per-user Run entry that starts Notchy at sign-in.
fn write_run_entry(enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe().ok();
    let data = match plan_run_entry(enabled, cfg!(debug_assertions), exe.as_deref()) {
        RunEntry::Write(exe) => {
            // Built before the key is opened, so a failure here cannot leak the handle.
            let command = format!("\"{}\"", exe.display());
            Some(
                command
                    .encode_utf16()
                    .chain(iter::once(0))
                    .flat_map(u16::to_le_bytes)
                    .collect::<Vec<u8>>(),
            )
        }
        RunEntry::Remove => None,
        RunEntry::Skip(reason) => {
            static LOGGED: Once = Once::new();
            LOGGED.call_once(|| eprintln!("notchy: {reason}"));
            return Ok(());
        }
    };

    let mut key = HKEY(std::ptr::null_mut());
    unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            None,
            KEY_SET_VALUE,
            &mut key,
        )
    }
    .ok()
    .map_err(|error| error.message())?;

    let result = match &data {
        Some(bytes) => unsafe { RegSetValueExW(key, w!("Notchy"), None, REG_SZ, Some(bytes)) },
        None => unsafe { RegDeleteValueW(key, w!("Notchy")) },
    };
    let _ = unsafe { RegCloseKey(key) };

    // Removing an entry that was never written already is the goal, not a failure.
    if data.is_none() && result == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    result.ok().map_err(|error| error.message())
}

/// Makes the Run entry match the setting. Runs at startup, so a moved exe is picked up.
pub fn sync_launch_at_login(enabled: bool) {
    if let Err(error) = write_run_entry(enabled) {
        eprintln!("notchy: could not update launch at login: {error}");
    }
}

#[tauri::command]
pub fn config_get(config: State<'_, SharedConfig>) -> Config {
    lock(&config).clone()
}

/// Sanitizes and saves `config`, applies what changed, and tells every window. Returns the saved
/// settings. A failed launch-at-login write is reported as an error after the save.
#[tauri::command]
pub async fn config_set(app: AppHandle, config: Config) -> Result<Config, String> {
    let next = config.sanitize();
    let shared = app.state::<SharedConfig>().inner().clone();
    let path = app.state::<ConfigFile>().0.clone();

    // Held through the write, so two saves cannot interleave on disk.
    let mut current = lock(&shared);
    let previous = current.clone();
    save(&path, &next).map_err(|error| format!("Could not save settings: {error}"))?;
    *current = next.clone();
    drop(current);

    let autostart = if previous.launch_at_login == next.launch_at_login {
        Ok(())
    } else {
        write_run_entry(next.launch_at_login)
    };
    if previous.lyrics != next.lyrics {
        lyrics::reapply(&app);
    }
    if previous.notifications != next.notifications {
        notifications::reapply(&app);
    }
    if previous.calendar != next.calendar {
        calendar::reapply(&app);
    }
    let _ = app.emit(UPDATE_EVENT, &next);
    autostart.map(|()| next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    /// A directory under the system temp dir, unique to this test run.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("notchy-config-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn defaults_are_already_sanitized() {
        assert_eq!(Config::default().sanitize(), Config::default());
    }

    #[test]
    fn sanitize_drops_unknown_and_repeated_pages() {
        let config = Config {
            pages: ids(&["controls", "nope", "media", "controls"]),
            ..Config::default()
        };
        assert_eq!(config.sanitize().pages, ids(&["controls", "media"]));
    }

    #[test]
    fn sanitize_keeps_at_least_one_page() {
        let none_known = Config {
            pages: ids(&["bogus"]),
            ..Config::default()
        };
        assert_eq!(
            none_known.sanitize().pages,
            ids(&["media", "controls", "timer", "stats", "calendar"])
        );

        let one = Config {
            pages: ids(&["media"]),
            ..Config::default()
        };
        assert_eq!(one.sanitize().pages, ids(&["media"]));
    }

    #[test]
    fn sanitize_drops_unknown_and_repeated_controls() {
        let config = Config {
            controls: ids(&["wifi", "made-up", "wifi", "mute"]),
            ..Config::default()
        };
        assert_eq!(config.sanitize().controls, ids(&["wifi", "mute"]));
    }

    #[test]
    fn sanitize_caps_tiles_and_leaves_sliders_alone() {
        let every = [
            "volume",
            "wifi",
            "bluetooth",
            "airplane",
            "mute",
            "awake",
            "lock",
            "screenshot",
            "mic",
            "darkmode",
            "brightness",
        ];
        let config = Config {
            controls: ids(&every),
            ..Config::default()
        };
        // Both sliders survive, and the first seven tiles in order follow.
        assert_eq!(
            config.sanitize().controls,
            ids(&[
                "volume",
                "wifi",
                "bluetooth",
                "airplane",
                "mute",
                "awake",
                "lock",
                "screenshot",
                "brightness",
            ])
        );
    }

    #[test]
    fn defaults_fit_under_the_cap() {
        assert_eq!(
            Config::default().sanitize().controls,
            Config::default().controls
        );
    }

    #[test]
    fn json_uses_camel_case_keys() {
        let json = serde_json::to_string(&Config::default()).unwrap();
        assert!(json.contains("\"confirmLock\":true"));
        assert!(json.contains("\"launchAtLogin\":false"));
        assert!(json.contains("\"pages\":[\"media\",\"controls\",\"timer\",\"stats\",\"calendar\"]"));
        assert!(json.contains("\"stats\":{\"enabled\":true}"));
        assert!(
            json.contains("\"calendar\":{\"enabled\":true,\"leadMinutes\":10,\"allDayHour\":9}")
        );
        assert!(json.contains("\"font\":\"inter\""));
        assert!(json.contains("\"hud\":true"));
        assert!(json.contains("\"timerSound\":true"));
        assert!(json.contains("\"idleClock\":true"));
        assert!(json.contains("\"focusMinutes\":25"));
        assert!(json.contains("\"shortBreakMinutes\":5"));
        assert!(json.contains("\"longBreakMinutes\":15"));
        assert!(json.contains("\"notifications\":{\"enabled\":true,\"deniedApps\":[]}"));
        assert!(json.contains(
            "\"battery\":{\"enabled\":true,\"chargeAlerts\":true,\"lowBatteryAlerts\":true}"
        ));
    }

    /// Files written before battery alerts existed have no such key. They get it on, with both
    /// kinds of alert on, and the rest of their settings are kept.
    #[test]
    fn file_without_battery_settings_turns_them_on() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert_eq!(config.sanitize().battery, Battery::default());

        let partial: Config =
            serde_json::from_str(r#"{"battery": {"lowBatteryAlerts": false}}"#).unwrap();
        assert_eq!(
            partial.battery,
            Battery {
                enabled: true,
                charge_alerts: true,
                low_battery_alerts: false,
            }
        );
    }

    /// Files written before the stats page existed have no such key. The stats setting is on, but
    /// the page is not in their page list, so it stays off until the user turns it on in Settings.
    #[test]
    fn file_without_stats_settings_keeps_the_page_off_and_the_setting_on() {
        let old: Config =
            serde_json::from_str(r#"{"pages": ["media", "controls", "timer"]}"#).unwrap();
        let old = old.sanitize();
        assert_eq!(old.pages, ids(&["media", "controls", "timer"]));
        assert_eq!(old.stats, Stats { enabled: true });

        let off: Config = serde_json::from_str(r#"{"stats": {"enabled": false}}"#).unwrap();
        assert!(!off.sanitize().stats.enabled);
    }

    /// Files written before notifications existed have no such key. They get it on, with nothing
    /// denied, and the rest of their settings are kept.
    #[test]
    fn file_without_notification_settings_turns_them_on() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert_eq!(config.sanitize().notifications, Notifications::default());

        let partial: Config =
            serde_json::from_str(r#"{"notifications": {"deniedApps": ["Mail"]}}"#).unwrap();
        let partial = partial.sanitize();
        assert!(partial.notifications.enabled);
        assert_eq!(partial.notifications.denied_apps, ids(&["Mail"]));
    }

    #[test]
    fn denied_apps_are_trimmed_and_repeats_dropped() {
        let config = Config {
            notifications: Notifications {
                enabled: false,
                denied_apps: ids(&["  Mail ", "", "Mail", "Chat"]),
            },
            ..Config::default()
        }
        .sanitize();
        assert!(!config.notifications.enabled);
        assert_eq!(config.notifications.denied_apps, ids(&["Mail", "Chat"]));
    }

    /// PowerShell writes a byte order mark. Before the fix, such a file parsed as nothing and every
    /// setting silently reset to its default.
    #[test]
    fn a_leading_byte_order_mark_does_not_reset_the_settings() {
        assert_eq!(strip_bom("\u{feff}{\"lyrics\": false}"), "{\"lyrics\": false}");
        assert_eq!(strip_bom("{\"lyrics\": false}"), "{\"lyrics\": false}");

        let dir = scratch("bom");
        fs::create_dir_all(&dir).unwrap();
        let path = file_in(&dir);
        fs::write(&path, "\u{feff}{\"lyrics\": false, \"timerSound\": false}").unwrap();
        let config = load(&path);
        assert!(!config.lyrics);
        assert!(!config.timer_sound);
        assert!(config.confirm_lock);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn pomodoro_minutes_are_clamped_to_their_bounds() {
        let config = Config {
            focus_minutes: 0,
            short_break_minutes: 500,
            long_break_minutes: 30,
            ..Config::default()
        }
        .sanitize();
        assert_eq!(config.focus_minutes, 1);
        assert_eq!(config.short_break_minutes, 60);
        assert_eq!(config.long_break_minutes, 30);

        let high = Config {
            focus_minutes: 999,
            ..Config::default()
        }
        .sanitize();
        assert_eq!(high.focus_minutes, 120);
    }

    #[test]
    fn file_without_timer_settings_gets_the_defaults() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        let config = config.sanitize();
        assert!(config.timer_sound);
        assert_eq!(
            (config.focus_minutes, config.short_break_minutes, config.long_break_minutes),
            (25, 5, 15)
        );
    }

    #[test]
    fn file_without_a_hud_setting_turns_it_on() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert!(config.hud);
        let off: Config = serde_json::from_str(r#"{"hud": false}"#).unwrap();
        assert!(!off.hud);
    }

    #[test]
    fn file_without_an_idle_clock_setting_shows_the_time() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert!(config.idle_clock);
        let off: Config = serde_json::from_str(r#"{"idleClock": false}"#).unwrap();
        assert!(!off.idle_clock);
    }

    #[test]
    fn sanitize_keeps_known_fonts_and_resets_unknown_ones() {
        for id in FONTS {
            let config = Config {
                font: id.to_string(),
                ..Config::default()
            };
            assert_eq!(config.sanitize().font, id);
        }

        let unknown = Config {
            font: "Comic Sans".to_string(),
            ..Config::default()
        };
        assert_eq!(unknown.sanitize().font, "inter");
    }

    #[test]
    fn file_without_a_font_gets_the_default() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert_eq!(config.sanitize().font, "inter");
    }

    #[test]
    fn partial_file_keeps_the_fields_it_has() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert!(!config.lyrics);
        assert!(config.confirm_lock);
        assert_eq!(config.pages, Config::default().pages);
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = scratch("missing");
        assert_eq!(load(&file_in(&dir)), Config::default());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = scratch("corrupt");
        fs::create_dir_all(&dir).unwrap();
        let path = file_in(&dir);
        fs::write(&path, "{ not json").unwrap();
        assert_eq!(load(&path), Config::default());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn saved_config_loads_back_and_leaves_no_temp_file() {
        let dir = scratch("roundtrip");
        let path = file_in(&dir);
        let config = Config {
            lyrics: false,
            pages: ids(&["controls"]),
            controls: ids(&["lock", "wifi"]),
            ..Config::default()
        };

        save(&path, &config).unwrap();
        assert_eq!(load(&path), config);
        assert!(!path.with_extension("json.tmp").exists());

        // A second save replaces the first.
        save(&path, &Config::default()).unwrap();
        assert_eq!(load(&path), Config::default());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn release_exe_is_registered_when_enabled() {
        let exe = Path::new(r"C:\Program Files\Notchy\notchy.exe");
        assert_eq!(
            plan_run_entry(true, false, Some(exe)),
            RunEntry::Write(exe.to_path_buf())
        );
        let built = Path::new(r"C:\claude-october-2026\notchy\src-tauri\target\release\notchy.exe");
        assert_eq!(
            plan_run_entry(true, false, Some(built)),
            RunEntry::Write(built.to_path_buf())
        );
    }

    #[test]
    fn disabling_removes_the_entry() {
        let exe = Path::new(r"C:\Program Files\Notchy\notchy.exe");
        assert_eq!(plan_run_entry(false, false, Some(exe)), RunEntry::Remove);
        assert_eq!(plan_run_entry(false, false, None), RunEntry::Remove);
    }

    #[test]
    fn debug_build_never_touches_the_entry() {
        let exe = Path::new(r"C:\Program Files\Notchy\notchy.exe");
        assert!(matches!(
            plan_run_entry(true, true, Some(exe)),
            RunEntry::Skip(_)
        ));
        assert!(matches!(
            plan_run_entry(false, true, Some(exe)),
            RunEntry::Skip(_)
        ));
        assert!(matches!(
            plan_run_entry(false, true, None),
            RunEntry::Skip(_)
        ));
    }

    #[test]
    fn debug_exe_is_refused_even_in_a_release_build() {
        let scratch = Path::new(
            r"C:\Users\me\AppData\Local\Temp\claude\x\scratchpad\app-target\debug\notchy.exe",
        );
        assert!(matches!(
            plan_run_entry(true, false, Some(scratch)),
            RunEntry::Skip(_)
        ));
        let target = Path::new(r"C:\repo\src-tauri\target\debug\notchy.exe");
        assert!(matches!(
            plan_run_entry(true, false, Some(target)),
            RunEntry::Skip(_)
        ));
    }

    #[test]
    fn missing_exe_path_is_not_written() {
        assert!(matches!(
            plan_run_entry(true, false, None),
            RunEntry::Skip(_)
        ));
    }

    #[test]
    fn dev_exe_detection_is_case_insensitive_and_exact() {
        assert!(is_dev_exe(Path::new(r"D:\x\Debug\notchy.exe")));
        assert!(is_dev_exe(Path::new(r"D:\x\TARGET\DEBUG\notchy.exe")));
        assert!(is_dev_exe(Path::new(r"D:\x\target\debug\deps\notchy.exe")));
        assert!(!is_dev_exe(Path::new(r"C:\Users\me\debugging\notchy.exe")));
        assert!(!is_dev_exe(Path::new(
            r"C:\Program Files\Notchy\notchy.exe"
        )));
        assert!(!is_dev_exe(Path::new(r"C:\repo\target\release\notchy.exe")));
        assert!(!is_dev_exe(Path::new(r"C:\debug-tools\notchy\notchy.exe")));
    }

    #[test]
    fn load_sanitizes_what_the_file_holds() {
        let dir = scratch("sanitized");
        fs::create_dir_all(&dir).unwrap();
        let path = file_in(&dir);
        fs::write(
            &path,
            r#"{"pages": ["nope"], "controls": ["wifi", "wifi", "x"]}"#,
        )
        .unwrap();
        let config = load(&path);
        assert_eq!(
            config.pages,
            ids(&["media", "controls", "timer", "stats", "calendar"])
        );
        assert_eq!(config.controls, ids(&["wifi"]));
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Files written before calendar existed have no such key. They get reminders on, at the
    /// defaults, and the rest of their settings are kept.
    #[test]
    fn file_without_calendar_settings_gets_the_defaults() {
        let config: Config = serde_json::from_str(r#"{"pages": ["media", "timer"]}"#).unwrap();
        let config = config.sanitize();
        assert_eq!(config.calendar, Calendar::default());
        assert_eq!(config.pages, ids(&["media", "timer"]));

        let partial: Config = serde_json::from_str(r#"{"calendar": {"leadMinutes": 30}}"#).unwrap();
        assert_eq!(
            partial.sanitize().calendar,
            Calendar {
                enabled: true,
                lead_minutes: 30,
                all_day_hour: 9,
            }
        );
    }

    #[test]
    fn calendar_lead_times_outside_the_choices_fall_back_to_ten_minutes() {
        for lead in LEAD_CHOICES {
            let config = Config {
                calendar: Calendar {
                    lead_minutes: lead,
                    ..Calendar::default()
                },
                ..Config::default()
            };
            assert_eq!(config.sanitize().calendar.lead_minutes, lead);
        }
        let odd = Config {
            calendar: Calendar {
                lead_minutes: 7,
                ..Calendar::default()
            },
            ..Config::default()
        };
        assert_eq!(odd.sanitize().calendar.lead_minutes, 10);
    }

    #[test]
    fn calendar_all_day_hour_is_held_to_a_clock_hour() {
        let late = Config {
            calendar: Calendar {
                all_day_hour: 40,
                ..Calendar::default()
            },
            ..Config::default()
        };
        assert_eq!(late.sanitize().calendar.all_day_hour, 23);
    }
}
