//! User settings, kept in `config.json` in the app config directory. The file is read once at
//! startup and written whole on every change. A missing or corrupt file gives the defaults.
//!
//! `config_set` is the only writer. It sanitizes, saves atomically, applies what changed (the
//! launch-at-login entry and lyrics), and emits `config://update` to every window.

use std::fs::{self, File};
use std::io::{self, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::ERROR_FILE_NOT_FOUND;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
    RegSetValueExW,
};
use windows::core::w;

use crate::modules::controls::{self, Kind};
use crate::modules::lyrics;

const FILE_NAME: &str = "config.json";
const UPDATE_EVENT: &str = "config://update";
/// Page ids the notch knows how to show, in their default order.
pub const PAGES: [&str; 2] = ["media", "controls"];
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
        }
    }
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

/// The settings file inside the app config directory.
pub fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE_NAME)
}

/// Reads the settings file. A missing or unreadable file, and invalid JSON, give the defaults.
/// A partial file keeps the fields it has.
pub fn load(path: &Path) -> Config {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Config>(&text).ok())
        .map_or_else(Config::default, Config::sanitize)
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

/// Writes or removes the per-user Run entry that starts Notchy at sign-in.
fn write_run_entry(enabled: bool) -> Result<(), String> {
    // Built before the key is opened, so a failure here cannot leak the handle.
    let data = if enabled {
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let command = format!("\"{}\"", exe.display());
        Some(
            command
                .encode_utf16()
                .chain(iter::once(0))
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<u8>>(),
        )
    } else {
        None
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
        assert_eq!(none_known.sanitize().pages, ids(&["media", "controls"]));

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
        assert!(json.contains("\"pages\":[\"media\",\"controls\"]"));
        assert!(json.contains("\"font\":\"inter\""));
        assert!(json.contains("\"hud\":true"));
    }

    #[test]
    fn file_without_a_hud_setting_turns_it_on() {
        let config: Config = serde_json::from_str(r#"{"lyrics": false}"#).unwrap();
        assert!(config.hud);
        let off: Config = serde_json::from_str(r#"{"hud": false}"#).unwrap();
        assert!(!off.hud);
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
        assert_eq!(config.pages, ids(&["media", "controls"]));
        assert_eq!(config.controls, ids(&["wifi"]));
        fs::remove_dir_all(&dir).unwrap();
    }
}
