//! Where events come from. `EventSource` is the seam. The service, the reminders and the island see
//! only `EventInstance`s, so a new source changes nothing outside this file and its own module.
//!
//! `LocalSource` is the editable one, kept in `events.json`. A read-only source (an imported ICS
//! calendar, later) implements `series` and inherits the rest. See `modules/calendar.rs` for how to
//! add one.

use std::path::PathBuf;

use time::PrimitiveDateTime;

use super::model::{CalendarEvent, EventInstance, LOCAL_SOURCE, instances};
use super::store;

/// One place events come from. `series` is the stored definitions, one per recurring or single
/// event. `events_between` turns those into occurrences, and the default does it for every source.
pub trait EventSource: Send {
    /// A stable id. Events name their source by it.
    fn id(&self) -> &str;

    /// Whether the user can change this source's events. A read-only source refuses `upsert` and
    /// `remove`.
    fn read_only(&self) -> bool;

    /// The events this source holds, one definition per series.
    fn series(&self) -> Vec<CalendarEvent>;

    /// The occurrences that overlap [from, to), in any order. A source that can fetch its own
    /// occurrences (a feed that expands its own rules) can replace this.
    fn events_between(&self, from: PrimitiveDateTime, to: PrimitiveDateTime) -> Vec<EventInstance> {
        let read_only = self.read_only();
        self.series()
            .iter()
            .flat_map(|event| instances(event, read_only, from, to))
            .collect()
    }

    /// Adds or replaces an event. Read-only sources refuse.
    fn upsert(&mut self, event: CalendarEvent) -> Result<(), String> {
        let _ = event;
        Err(read_only_message(self.id()))
    }

    /// Removes an event by id. Read-only sources refuse.
    fn remove(&mut self, id: &str) -> Result<(), String> {
        let _ = id;
        Err(read_only_message(self.id()))
    }
}

fn read_only_message(source: &str) -> String {
    format!("The calendar {source} is read-only.")
}

/// The events the user makes in Notchy, kept in `events.json`. Every change is saved at once. A
/// change that cannot be saved is undone, so the list in memory matches the file.
pub struct LocalSource {
    path: PathBuf,
    events: Vec<CalendarEvent>,
}

impl LocalSource {
    /// Reads the events file. A missing or unusable file gives an empty calendar.
    pub fn open(path: PathBuf) -> Self {
        let events = store::load_events(&path);
        Self { path, events }
    }

    fn save(&self) -> Result<(), String> {
        store::save_events(&self.path, &self.events)
            .map_err(|error| format!("Could not save the calendar: {error}"))
    }
}

impl EventSource for LocalSource {
    fn id(&self) -> &str {
        LOCAL_SOURCE
    }

    fn read_only(&self) -> bool {
        false
    }

    fn series(&self) -> Vec<CalendarEvent> {
        self.events.clone()
    }

    fn upsert(&mut self, event: CalendarEvent) -> Result<(), String> {
        let previous = self.events.clone();
        match self.events.iter().position(|held| held.id == event.id) {
            Some(index) => self.events[index] = event,
            None => self.events.push(event),
        }
        self.save().inspect_err(|_| self.events = previous)
    }

    fn remove(&mut self, id: &str) -> Result<(), String> {
        let previous = self.events.clone();
        let before = self.events.len();
        self.events.retain(|held| held.id != id);
        if self.events.len() == before {
            return Err("That event no longer exists.".to_string());
        }
        self.save().inspect_err(|_| self.events = previous)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::calendar::model::{LocalDateTime, Recurrence};
    use time::macros::datetime;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "notchy-calendar-source-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn event(id: &str, start: PrimitiveDateTime) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            source: LOCAL_SOURCE.into(),
            title: format!("Event {id}"),
            notes: String::new(),
            start: LocalDateTime(start),
            all_day: false,
            duration_minutes: None,
            recurrence: Recurrence::None,
            remind_before_minutes: None,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    #[test]
    fn upsert_adds_then_replaces_by_id_and_survives_a_reopen() {
        let dir = scratch("upsert");
        let path = dir.join(store::EVENTS_FILE);
        let mut source = LocalSource::open(path.clone());
        source
            .upsert(event("a", datetime!(2026-10-23 10:00)))
            .unwrap();
        source
            .upsert(event("a", datetime!(2026-10-24 11:00)))
            .unwrap();
        assert_eq!(source.series().len(), 1);

        let reopened = LocalSource::open(path);
        assert_eq!(
            reopened.series(),
            vec![event("a", datetime!(2026-10-24 11:00))]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remove_drops_the_event_and_reports_a_missing_one() {
        let dir = scratch("remove");
        let mut source = LocalSource::open(dir.join(store::EVENTS_FILE));
        source
            .upsert(event("a", datetime!(2026-10-23 10:00)))
            .unwrap();
        source.remove("a").unwrap();
        assert!(source.series().is_empty());
        assert_eq!(
            source.remove("a").unwrap_err(),
            "That event no longer exists."
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_save_undoes_the_change() {
        // The events file cannot be written where a directory sits, so the save fails.
        let dir = scratch("failed-save");
        let path = dir.join("blocked");
        std::fs::create_dir_all(path.join(store::EVENTS_FILE)).unwrap();
        let mut source = LocalSource::open(path.join(store::EVENTS_FILE));
        assert!(
            source
                .upsert(event("a", datetime!(2026-10-23 10:00)))
                .is_err()
        );
        assert!(source.series().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_local_source_expands_its_series_through_the_trait() {
        let dir = scratch("expand");
        let mut source = LocalSource::open(dir.join(store::EVENTS_FILE));
        let mut daily = event("a", datetime!(2026-10-23 10:00));
        daily.recurrence = Recurrence::Daily;
        source.upsert(daily).unwrap();
        let found = source.events_between(datetime!(2026-10-23 00:00), datetime!(2026-10-26 00:00));
        assert_eq!(found.len(), 3);
        assert!(found.iter().all(|instance| !instance.read_only));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
