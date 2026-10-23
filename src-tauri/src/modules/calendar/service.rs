//! The calendar's one front door. It owns every source, merges what they hold, and routes each change
//! to the source that owns the event. The scheduler and the island read only the merged, sorted list
//! from here, so a new source never reaches them.

use std::cmp::Ordering;

use time::{Duration, PrimitiveDateTime};

use super::model::{CalendarEvent, EventInstance};
use super::source::EventSource;

/// How far ahead the island looks for upcoming events. A year covers every yearly event once.
const HORIZON_DAYS: i64 = 366;

pub struct CalendarService {
    sources: Vec<Box<dyn EventSource>>,
}

impl CalendarService {
    pub fn new(sources: Vec<Box<dyn EventSource>>) -> Self {
        Self { sources }
    }

    /// Every series from every source, in start order.
    pub fn events(&self) -> Vec<CalendarEvent> {
        let mut events: Vec<CalendarEvent> = self
            .sources
            .iter()
            .flat_map(|source| source.series())
            .collect();
        events.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.title.cmp(&b.title)));
        events
    }

    /// The occurrences from every source that overlap [from, to), sorted by start.
    pub fn between(&self, from: PrimitiveDateTime, to: PrimitiveDateTime) -> Vec<EventInstance> {
        let mut found: Vec<EventInstance> = self
            .sources
            .iter()
            .flat_map(|source| source.events_between(from, to))
            .collect();
        found.sort_by(compare_instances);
        found
    }

    /// The first `count` occurrences from `now` onwards, sorted by start.
    pub fn upcoming(&self, now: PrimitiveDateTime, count: usize) -> Vec<EventInstance> {
        let mut found = self.between(now, now + Duration::days(HORIZON_DAYS));
        found.truncate(count);
        found
    }

    /// Adds or replaces an event in the source it names. A read-only source refuses.
    pub fn upsert(&mut self, event: CalendarEvent) -> Result<(), String> {
        self.source_mut(&event.source)?.upsert(event)
    }

    /// Removes an event from the source it belongs to.
    pub fn remove(&mut self, source: &str, id: &str) -> Result<(), String> {
        self.source_mut(source)?.remove(id)
    }

    fn source_mut(&mut self, id: &str) -> Result<&mut Box<dyn EventSource>, String> {
        self.sources
            .iter_mut()
            .find(|source| source.id() == id)
            .ok_or_else(|| "That calendar is not available.".to_string())
    }
}

/// Start, then title, then event id, so the order is stable when two events start together.
fn compare_instances(a: &EventInstance, b: &EventInstance) -> Ordering {
    a.start
        .cmp(&b.start)
        .then_with(|| a.title.cmp(&b.title))
        .then_with(|| a.event_id.cmp(&b.event_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::calendar::model::{LOCAL_SOURCE, LocalDateTime, Recurrence, day_range};
    use crate::modules::calendar::source::LocalSource;
    use time::macros::datetime;

    /// A read-only source, the way an imported calendar will be. It proves the seam: the service
    /// merges it with the local source and refuses to change it, and nothing else is needed.
    struct FakeFeed {
        events: Vec<CalendarEvent>,
    }

    impl EventSource for FakeFeed {
        fn id(&self) -> &str {
            "feed:test"
        }

        fn read_only(&self) -> bool {
            true
        }

        fn series(&self) -> Vec<CalendarEvent> {
            self.events.clone()
        }
    }

    fn event(id: &str, source: &str, title: &str, start: PrimitiveDateTime) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            source: source.into(),
            title: title.into(),
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

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "notchy-calendar-service-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn service(dir: &std::path::Path) -> CalendarService {
        let feed = FakeFeed {
            events: vec![event(
                "f1",
                "feed:test",
                "Team call",
                datetime!(2026-10-23 09:00),
            )],
        };
        let local = LocalSource::open(dir.join(crate::modules::calendar::store::EVENTS_FILE));
        CalendarService::new(vec![Box::new(local), Box::new(feed)])
    }

    #[test]
    fn two_sources_merge_into_one_list_sorted_by_start() {
        let dir = scratch("merge");
        let mut service = service(&dir);
        service
            .upsert(event(
                "l1",
                LOCAL_SOURCE,
                "Lunch",
                datetime!(2026-10-23 12:00),
            ))
            .unwrap();
        service
            .upsert(event(
                "l0",
                LOCAL_SOURCE,
                "Coffee",
                datetime!(2026-10-23 08:00),
            ))
            .unwrap();

        let titles: Vec<String> = service
            .between(datetime!(2026-10-23 00:00), datetime!(2026-10-24 00:00))
            .into_iter()
            .map(|instance| instance.title)
            .collect();
        assert_eq!(titles, ["Coffee", "Team call", "Lunch"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn instances_say_whether_their_source_is_read_only() {
        let dir = scratch("flags");
        let service = service(&dir);
        let found = service.between(datetime!(2026-10-23 00:00), datetime!(2026-10-24 00:00));
        assert_eq!(found.len(), 1);
        assert!(found[0].read_only);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_read_only_source_refuses_changes_and_the_local_one_accepts_them() {
        let dir = scratch("refuse");
        let mut service = service(&dir);
        let refused = service.upsert(event(
            "f2",
            "feed:test",
            "Edit me",
            datetime!(2026-10-23 10:00),
        ));
        assert_eq!(refused.unwrap_err(), "The calendar feed:test is read-only.");
        assert!(service.remove("feed:test", "f1").is_err());
        assert!(
            service
                .upsert(event(
                    "l9",
                    LOCAL_SOURCE,
                    "Mine",
                    datetime!(2026-10-23 10:00)
                ))
                .is_ok()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unknown_source_is_an_error_not_a_panic() {
        let dir = scratch("unknown");
        let mut service = service(&dir);
        let error = service
            .upsert(event("x", "ics:nope", "Ghost", datetime!(2026-10-23 10:00)))
            .unwrap_err();
        assert_eq!(error, "That calendar is not available.");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn upcoming_skips_what_has_ended_and_takes_the_first_few() {
        let dir = scratch("upcoming");
        let mut service = service(&dir);
        for (id, hour) in [("a", 10), ("b", 11), ("c", 12), ("d", 13), ("e", 14)] {
            let start = PrimitiveDateTime::new(
                datetime!(2026-10-23 00:00).date(),
                time::Time::from_hms(hour, 0, 0).unwrap(),
            );
            service
                .upsert(event(id, LOCAL_SOURCE, &format!("Event {id}"), start))
                .unwrap();
        }
        let titles: Vec<String> = service
            .upcoming(datetime!(2026-10-23 10:30), 3)
            .into_iter()
            .map(|instance| instance.title)
            .collect();
        // The 10:00 event has no length, so it is over once its start has passed.
        assert_eq!(titles, ["Event b", "Event c", "Event d"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn events_lists_every_series_from_every_source() {
        let dir = scratch("events");
        let mut service = service(&dir);
        service
            .upsert(event(
                "l1",
                LOCAL_SOURCE,
                "Lunch",
                datetime!(2026-10-23 12:00),
            ))
            .unwrap();
        let ids: Vec<String> = service.events().into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["f1", "l1"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A service that holds only the local source, for the range tests.
    fn local_only(dir: &std::path::Path) -> CalendarService {
        let local = LocalSource::open(dir.join(crate::modules::calendar::store::EVENTS_FILE));
        CalendarService::new(vec![Box::new(local)])
    }

    /// The occurrences the calendar command returns for the days `from` to `to`, as "MM-DD HH:MM Title".
    fn range(service: &CalendarService, from: &str, to: &str) -> Vec<String> {
        let (from, to) = day_range(from, to).unwrap();
        service
            .between(from, to)
            .into_iter()
            .map(|i| format!("{:02}-{:02} {:02}:{:02} {}", i.start.0.month() as u8, i.start.0.day(), i.start.0.hour(), i.start.0.minute(), i.title))
            .collect()
    }

    #[test]
    fn a_recurring_event_lands_on_every_day_of_the_grid_it_falls_on() {
        let dir = scratch("range-daily");
        let mut service = local_only(&dir);
        let mut standup = event("s", LOCAL_SOURCE, "Standup", datetime!(2026-10-01 09:00));
        standup.recurrence = Recurrence::Daily;
        service.upsert(standup).unwrap();

        // A six-week grid from 28 September to 8 November: 42 days, and the series starts on the 1st.
        let found = range(&service, "2026-09-28", "2026-11-08");
        assert_eq!(found.len(), 39);
        assert_eq!(found.first().unwrap(), "10-01 09:00 Standup");
        assert_eq!(found.last().unwrap(), "11-08 09:00 Standup");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn events_on_the_first_and_last_day_are_in_and_their_neighbours_are_out() {
        let dir = scratch("range-edges");
        let mut service = local_only(&dir);
        for (id, start) in [
            ("before", datetime!(2026-09-30 23:59)),
            ("first", datetime!(2026-10-01 00:30)),
            ("last", datetime!(2026-10-31 23:30)),
            ("after", datetime!(2026-11-01 00:00)),
        ] {
            service
                .upsert(event(id, LOCAL_SOURCE, id, start))
                .unwrap();
        }
        assert_eq!(
            range(&service, "2026-10-01", "2026-10-31"),
            ["10-01 00:30 first", "10-31 23:30 last"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn all_day_events_on_the_first_and_last_day_are_in_and_neighbours_are_out() {
        let dir = scratch("range-all-day");
        let mut service = local_only(&dir);
        for (id, start) in [
            ("before", datetime!(2026-09-30 00:00)),
            ("first", datetime!(2026-10-01 00:00)),
            ("last", datetime!(2026-10-31 00:00)),
            ("after", datetime!(2026-11-01 00:00)),
        ] {
            let mut day = event(id, LOCAL_SOURCE, id, start);
            day.all_day = true;
            service.upsert(day).unwrap();
        }
        assert_eq!(
            range(&service, "2026-10-01", "2026-10-31"),
            ["10-01 00:00 first", "10-31 00:00 last"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_empty_range_and_an_empty_calendar_give_nothing() {
        let dir = scratch("range-empty");
        let mut service = local_only(&dir);
        assert!(range(&service, "2026-10-01", "2026-10-31").is_empty());
        service
            .upsert(event("a", LOCAL_SOURCE, "Far away", datetime!(2030-01-01 10:00)))
            .unwrap();
        assert!(range(&service, "2026-10-01", "2026-10-31").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_monthly_event_on_the_31st_lands_on_the_last_day_of_november_and_a_late_event_spills_into_the_next_day() {
        let dir = scratch("range-month-end");
        let mut service = local_only(&dir);
        let mut rent = event("r", LOCAL_SOURCE, "Rent", datetime!(2026-01-31 12:00));
        rent.recurrence = Recurrence::Monthly;
        service.upsert(rent).unwrap();
        let mut late = event("l", LOCAL_SOURCE, "Late", datetime!(2026-10-31 23:30));
        late.duration_minutes = Some(60);
        service.upsert(late).unwrap();

        // The late event runs past midnight, so it overlaps 1 November and is found there.
        assert_eq!(
            range(&service, "2026-11-01", "2026-11-30"),
            ["10-31 23:30 Late", "11-30 12:00 Rent"]
        );
        assert_eq!(
            range(&service, "2026-11-01", "2026-11-01"),
            ["10-31 23:30 Late"]
        );
        assert_eq!(
            range(&service, "2026-10-31", "2026-10-31"),
            ["10-31 12:00 Rent", "10-31 23:30 Late"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
