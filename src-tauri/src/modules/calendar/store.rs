//! The calendar's files, next to `config.json`. Both are versioned and written through a temp file and
//! a rename, like the settings. Neither is in `config.json`, because the settings window saves that
//! whole file and would overwrite the events.
//!
//! - `events.json` holds the events the user made. A missing file is an empty calendar. A file that
//!   cannot be read, or has another version, is moved to `events.json.bak` and the calendar starts
//!   empty, so nothing is silently lost.
//! - `calendar-reminders.json` holds the reminders already shown, so a restart does not show them
//!   again. Losing it is harmless beyond a repeat, so a bad file just starts empty.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};
use time::PrimitiveDateTime;

use super::model::{CalendarEvent, LocalDateTime};

pub const EVENTS_FILE: &str = "events.json";
pub const FIRED_FILE: &str = "calendar-reminders.json";
/// The version written to both files. Read back only when it matches.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsFile {
    version: u32,
    events: Vec<CalendarEvent>,
}

#[derive(Serialize, Deserialize)]
struct Fired {
    key: String,
    at: LocalDateTime,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FiredFile {
    version: u32,
    fired: Vec<Fired>,
}

/// The text without a leading byte order mark, which Windows PowerShell writes.
fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

/// The events in the file. Missing gives none. Unreadable, or another version, is set aside as
/// `.bak` first, and the calendar starts empty.
pub fn load_events(path: &Path) -> Vec<CalendarEvent> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(_) => {
            set_aside(path);
            return Vec::new();
        }
    };
    match serde_json::from_str::<EventsFile>(strip_bom(&text)) {
        Ok(file) if file.version == VERSION => file.events,
        _ => {
            set_aside(path);
            Vec::new()
        }
    }
}

/// Moves a file that cannot be used to `<name>.bak`, replacing an older backup. Best effort: if the
/// move fails the file is left where it is, and the next save overwrites it.
fn set_aside(path: &Path) {
    let backup = path.with_extension("json.bak");
    let _ = fs::remove_file(&backup);
    if let Err(error) = fs::rename(path, &backup) {
        eprintln!("notchy: could not set aside {}: {error}", path.display());
    }
}

/// Writes the events through a temp file and a rename, so a crash never leaves half a file behind.
pub fn save_events(path: &Path, events: &[CalendarEvent]) -> io::Result<()> {
    let file = EventsFile {
        version: VERSION,
        events: events.to_vec(),
    };
    write_json(path, &file)
}

/// The reminders already shown, by key. Missing or unreadable gives none.
pub fn load_fired(path: &Path) -> BTreeMap<String, PrimitiveDateTime> {
    let Ok(text) = fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<FiredFile>(strip_bom(&text)) {
        Ok(file) if file.version == VERSION => file
            .fired
            .into_iter()
            .map(|fired| (fired.key, fired.at.0))
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// Saves the reminders already shown.
pub fn save_fired(path: &Path, fired: &BTreeMap<String, PrimitiveDateTime>) -> io::Result<()> {
    let file = FiredFile {
        version: VERSION,
        fired: fired
            .iter()
            .map(|(key, at)| Fired {
                key: key.clone(),
                at: LocalDateTime(*at),
            })
            .collect(),
    };
    write_json(path, &file)
}

fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::calendar::model::{LOCAL_SOURCE, Recurrence};
    use time::macros::datetime;

    /// A directory under the system temp dir, unique to this test run.
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "notchy-calendar-store-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn event(id: &str) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            source: LOCAL_SOURCE.into(),
            title: "Dentist".into(),
            notes: "Bring the card".into(),
            start: LocalDateTime(datetime!(2026-10-23 14:30)),
            all_day: false,
            duration_minutes: Some(45),
            recurrence: Recurrence::Weekly,
            remind_before_minutes: Some(vec![5, 30]),
            created_ms: 1,
            updated_ms: 2,
        }
    }

    #[test]
    fn events_round_trip_and_leave_no_temp_file() {
        let dir = scratch("round-trip");
        let path = dir.join(EVENTS_FILE);
        let events = vec![event("a"), event("b")];
        save_events(&path, &events).unwrap();
        assert_eq!(load_events(&path), events);
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_events_file_is_an_empty_calendar() {
        let dir = scratch("missing");
        assert!(load_events(&dir.join(EVENTS_FILE)).is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_events_file_is_set_aside_and_the_calendar_starts_empty() {
        let dir = scratch("corrupt");
        let path = dir.join(EVENTS_FILE);
        fs::write(&path, "{ not json").unwrap();
        assert!(load_events(&path).is_empty());
        assert!(!path.exists());
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            "{ not json"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unreadable_events_file_is_set_aside() {
        let dir = scratch("invalid-utf8");
        let path = dir.join(EVENTS_FILE);
        let bytes = [0xFF, 0xFE, 0x00];
        fs::write(&path, bytes).unwrap();
        assert!(load_events(&path).is_empty());
        assert_eq!(fs::read(path.with_extension("json.bak")).unwrap(), bytes);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_second_corrupt_file_replaces_the_old_backup() {
        let dir = scratch("second-backup");
        let path = dir.join(EVENTS_FILE);
        fs::write(&path, "first").unwrap();
        load_events(&path);
        fs::write(&path, "second").unwrap();
        load_events(&path);
        assert_eq!(
            fs::read_to_string(path.with_extension("json.bak")).unwrap(),
            "second"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn another_version_is_set_aside_rather_than_misread() {
        let dir = scratch("version");
        let path = dir.join(EVENTS_FILE);
        fs::write(&path, r#"{"version": 2, "events": []}"#).unwrap();
        assert!(load_events(&path).is_empty());
        assert!(path.with_extension("json.bak").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_byte_order_mark_does_not_lose_the_events() {
        let dir = scratch("bom");
        let path = dir.join(EVENTS_FILE);
        let json = serde_json::to_string(&EventsFile {
            version: VERSION,
            events: vec![event("a")],
        })
        .unwrap();
        fs::write(&path, format!("\u{feff}{json}")).unwrap();
        assert_eq!(load_events(&path), vec![event("a")]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn fired_reminders_round_trip() {
        let dir = scratch("fired");
        let path = dir.join(FIRED_FILE);
        let mut fired = BTreeMap::new();
        fired.insert(
            "a|2026-10-23T14:30|10".to_string(),
            datetime!(2026-10-23 14:20),
        );
        save_fired(&path, &fired).unwrap();
        assert_eq!(load_fired(&path), fired);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_corrupt_fired_file_starts_empty_without_a_backup() {
        let dir = scratch("fired-corrupt");
        let path = dir.join(FIRED_FILE);
        fs::write(&path, "[").unwrap();
        assert!(load_fired(&path).is_empty());
        assert!(path.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
