//! The calendar's domain model. Pure: no clock, no files, no Tauri, so every rule is tested with
//! fixed dates.
//!
//! Times are local, naive date-times. A user who types 14:30 gets 14:30 on this PC, and nothing is
//! stored in UTC, so a recurring event keeps its wall-clock time across a DST change. A time that
//! falls in the skipped spring-forward hour is treated as the wall-clock time; its reminder may be
//! skipped when the jump makes it overdue by `OVERDUE_LIMIT` or more. On the autumn night a repeated
//! hour fires once, because reminders are keyed by the wall-clock time.
//!
//! A series is one stored `CalendarEvent`. `instances` expands it into the occurrences that overlap a
//! window. Every occurrence is computed from the series start, not from the one before, so a monthly
//! event on the 31st lands on the last day of a short month and returns to the 31st afterwards.

use std::fmt;
use std::hash::{BuildHasher, Hasher, RandomState};

use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month, PrimitiveDateTime, Time};

/// The source that owns the events the user makes in Notchy.
pub const LOCAL_SOURCE: &str = "local";
/// The longest title and notes accepted, in characters.
pub const MAX_TITLE_CHARS: usize = 200;
pub const MAX_NOTES_CHARS: usize = 2000;

/// A local date and time, held to the minute. Stored and sent as "YYYY-MM-DDTHH:MM".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalDateTime(pub PrimitiveDateTime);

impl LocalDateTime {
    /// Reads "YYYY-MM-DDTHH:MM". None for anything else, including a day that does not exist.
    pub fn parse(text: &str) -> Option<Self> {
        let (date, time) = text.split_once('T')?;
        let mut parts = date.split('-');
        let year: i32 = parts.next()?.parse().ok()?;
        let month: u8 = parts.next()?.parse().ok()?;
        let day: u8 = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        let (hour, minute) = time.split_once(':')?;
        let date = Date::from_calendar_date(year, Month::try_from(month).ok()?, day).ok()?;
        let time = Time::from_hms(hour.parse().ok()?, minute.parse().ok()?, 0).ok()?;
        Some(Self(PrimitiveDateTime::new(date, time)))
    }
}

impl fmt::Display for LocalDateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = self.0;
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}",
            t.year(),
            t.month() as u8,
            t.day(),
            t.hour(),
            t.minute()
        )
    }
}

impl Serialize for LocalDateTime {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for LocalDateTime {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| <D::Error as serde::de::Error>::custom(format!("bad date-time {text}")))
    }
}

/// How often a series repeats. `None` is a single event. A later rule (an RFC 5545 RRULE) would be
/// one more variant holding its text, expanded in `occurrence_start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Recurrence {
    #[default]
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

/// An event as it is stored: one series, with its start and how it repeats. Mirrors `CalendarEvent`
/// in src/lib/calendar.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarEvent {
    pub id: String,
    /// The source that holds the event. `LOCAL_SOURCE` for events the user made in Notchy.
    pub source: String,
    pub title: String,
    pub notes: String,
    /// The first occurrence. For an all-day event the time is midnight and is not shown.
    pub start: LocalDateTime,
    pub all_day: bool,
    /// Length in minutes. None is a moment with no end.
    pub duration_minutes: Option<u32>,
    pub recurrence: Recurrence,
    /// Minutes before the start to remind at. None inherits the global lead time in Settings.
    pub remind_before_minutes: Option<Vec<u32>>,
    pub created_ms: u64,
    pub updated_ms: u64,
}

/// One occurrence of an event, the only thing the scheduler and the island see. Mirrors
/// `EventInstance` in src/lib/calendar.ts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInstance {
    pub event_id: String,
    pub source: String,
    pub title: String,
    pub notes: String,
    pub start: LocalDateTime,
    pub end: LocalDateTime,
    pub all_day: bool,
    /// True for a source the user cannot edit, such as an imported calendar.
    pub read_only: bool,
    pub remind_before_minutes: Option<Vec<u32>>,
}

/// The form the settings window sends. `start` is "YYYY-MM-DDTHH:MM" and is checked by `check`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventInput {
    /// The event being edited. None adds a new one.
    pub id: Option<String>,
    pub title: String,
    pub notes: String,
    pub start: String,
    pub all_day: bool,
    pub recurrence: Recurrence,
}

/// The checked fields of an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub title: String,
    pub notes: String,
    pub start: LocalDateTime,
}

/// Checks a form before it is stored. The title is trimmed and must not be empty. The start must be
/// a real date and time. An all-day event is pinned to midnight. Errors are for the person to read.
pub fn check(input: &EventInput) -> Result<Checked, String> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err("Give the event a title.".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!(
            "The title is longer than {MAX_TITLE_CHARS} characters."
        ));
    }
    if input.notes.chars().count() > MAX_NOTES_CHARS {
        return Err(format!(
            "The notes are longer than {MAX_NOTES_CHARS} characters."
        ));
    }
    let start = LocalDateTime::parse(input.start.trim())
        .ok_or_else(|| "Pick a valid date and time.".to_string())?;
    let start = if input.all_day {
        LocalDateTime(PrimitiveDateTime::new(start.0.date(), Time::MIDNIGHT))
    } else {
        start
    };
    Ok(Checked {
        title: title.to_string(),
        notes: input.notes.clone(),
        start,
    })
}

/// A new event id: 32 hex digits. The std hash seed is random per instance, so no crate is needed.
pub fn new_id(now_ms: u64) -> String {
    let mut first = RandomState::new().build_hasher();
    first.write_u64(now_ms);
    let mut second = RandomState::new().build_hasher();
    second.write_u64(first.finish());
    format!("{:016x}{:016x}", first.finish(), second.finish())
}

/// How long an occurrence lasts. An all-day event covers its whole day.
pub fn span(event: &CalendarEvent) -> Duration {
    if event.all_day {
        Duration::days(1)
    } else {
        Duration::minutes(i64::from(event.duration_minutes.unwrap_or(0)))
    }
}

/// The start of occurrence `k` of a series that starts at `start`. Occurrence 0 is the start itself.
/// A monthly or yearly day that a month lacks (the 31st, 29 February) falls on the last day of it.
pub fn occurrence_start(
    start: PrimitiveDateTime,
    recurrence: Recurrence,
    k: i64,
) -> Option<PrimitiveDateTime> {
    let date = start.date();
    let next = match recurrence {
        Recurrence::None => (k == 0).then_some(date)?,
        Recurrence::Daily => date.checked_add(Duration::days(k))?,
        Recurrence::Weekly => date.checked_add(Duration::weeks(k))?,
        Recurrence::Monthly => {
            let index = i64::from(date.year()) * 12 + i64::from(date.month() as u8 - 1) + k;
            let year = i32::try_from(index.div_euclid(12)).ok()?;
            let month = Month::try_from((index.rem_euclid(12) + 1) as u8).ok()?;
            Date::from_calendar_date(year, month, date.day().min(month.length(year))).ok()?
        }
        Recurrence::Yearly => {
            let year = date.year().checked_add(i32::try_from(k).ok()?)?;
            let month = date.month();
            Date::from_calendar_date(year, month, date.day().min(month.length(year))).ok()?
        }
    };
    Some(PrimitiveDateTime::new(next, start.time()))
}

/// A lower bound on the occurrence index that can still overlap `probe`. Taking one step back from
/// the first occurrence that is due, so a clamped day never skips one.
fn first_index(start: PrimitiveDateTime, recurrence: Recurrence, probe: PrimitiveDateTime) -> i64 {
    let gap = match recurrence {
        Recurrence::None => return 0,
        Recurrence::Daily => (probe.date() - start.date()).whole_days(),
        Recurrence::Weekly => (probe.date() - start.date()).whole_days() / 7,
        Recurrence::Monthly => {
            i64::from(probe.year() - start.year()) * 12 + i64::from(probe.month() as u8)
                - i64::from(start.month() as u8)
        }
        Recurrence::Yearly => i64::from(probe.year() - start.year()),
    };
    (gap - 1).max(0)
}

/// The start of each occurrence that overlaps the window [from, to), in order. An occurrence that
/// ends exactly at `from` does not overlap. One that starts exactly at `to` does not either. A
/// moment with no length counts when it starts inside the window.
pub fn occurrence_starts(
    event: &CalendarEvent,
    from: PrimitiveDateTime,
    to: PrimitiveDateTime,
) -> Vec<PrimitiveDateTime> {
    let start = event.start.0;
    let span = span(event);
    let probe = from.checked_sub(span).unwrap_or(from);
    let mut k = first_index(start, event.recurrence, probe);
    let mut starts = Vec::new();
    while let Some(occurrence) = occurrence_start(start, event.recurrence, k) {
        if occurrence >= to {
            break;
        }
        if occurrence >= from || occurrence.checked_add(span).is_some_and(|end| end > from) {
            starts.push(occurrence);
        }
        k += 1;
    }
    starts
}

/// The occurrences of `event` that overlap [from, to), as instances.
pub fn instances(
    event: &CalendarEvent,
    read_only: bool,
    from: PrimitiveDateTime,
    to: PrimitiveDateTime,
) -> Vec<EventInstance> {
    let span = span(event);
    occurrence_starts(event, from, to)
        .into_iter()
        .map(|start| EventInstance {
            event_id: event.id.clone(),
            source: event.source.clone(),
            title: event.title.clone(),
            notes: event.notes.clone(),
            start: LocalDateTime(start),
            end: LocalDateTime(start.checked_add(span).unwrap_or(start)),
            all_day: event.all_day,
            read_only,
            remind_before_minutes: event.remind_before_minutes.clone(),
        })
        .collect()
}

/// The longest day range the grid can ask for, in days. A month grid shows 42 days.
pub const MAX_RANGE_DAYS: i64 = 366;

/// The window for two local dates, "YYYY-MM-DD". Both days are included: the window starts at the
/// midnight of `from` and ends at the midnight after `to`. Errors are for the person to read.
pub fn day_range(from: &str, to: &str) -> Result<(PrimitiveDateTime, PrimitiveDateTime), String> {
    let first = parse_day(from)?;
    let last = parse_day(to)?;
    let end = last
        .next_day()
        .ok_or_else(|| format!("Pick a valid date: {to}."))?;
    if end <= first {
        return Err("The range ends before it starts.".to_string());
    }
    if (end - first).whole_days() > MAX_RANGE_DAYS {
        return Err(format!("The range is longer than {MAX_RANGE_DAYS} days."));
    }
    Ok((
        PrimitiveDateTime::new(first, Time::MIDNIGHT),
        PrimitiveDateTime::new(end, Time::MIDNIGHT),
    ))
}

/// Reads "YYYY-MM-DD" exactly. Anything else, including a day that does not exist, is an error.
fn parse_day(text: &str) -> Result<Date, String> {
    let bad = || format!("Pick a valid date: {text}.");
    if text.len() != 10 {
        return Err(bad());
    }
    let mut parts = text.split('-');
    let year: i32 = parts.next().and_then(|part| part.parse().ok()).ok_or_else(bad)?;
    let month: u8 = parts.next().and_then(|part| part.parse().ok()).ok_or_else(bad)?;
    let day: u8 = parts.next().and_then(|part| part.parse().ok()).ok_or_else(bad)?;
    if parts.next().is_some() {
        return Err(bad());
    }
    let month = Month::try_from(month).map_err(|_| bad())?;
    Date::from_calendar_date(year, month, day).map_err(|_| bad())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_day_range_includes_both_of_its_days() {
        let (from, to) = day_range("2026-10-01", "2026-10-31").unwrap();
        assert_eq!(from, datetime!(2026-10-01 00:00));
        assert_eq!(to, datetime!(2026-11-01 00:00));
        let (from, to) = day_range("2026-10-23", "2026-10-23").unwrap();
        assert_eq!(from, datetime!(2026-10-23 00:00));
        assert_eq!(to, datetime!(2026-10-24 00:00));
    }

    #[test]
    fn a_day_range_refuses_bad_dates_reversed_ranges_and_long_ones() {
        assert!(day_range("2026-02-30", "2026-03-01").is_err());
        assert!(day_range("2026-10-1", "2026-10-02").is_err());
        assert!(day_range("2026/10/01", "2026-10-02").is_err());
        assert!(day_range("2026-10-01T00:00", "2026-10-02").is_err());
        assert!(day_range("2026-10-05", "2026-10-04").is_err());
        assert!(day_range("2026-01-01", "2026-12-31").is_ok());
        assert!(day_range("2026-01-01", "2027-01-01").is_ok());
        assert!(day_range("2026-01-01", "2027-01-02").is_err());
    }

    fn ymd(year: i32, month: u8, day: u8) -> Date {
        Date::from_calendar_date(year, Month::try_from(month).unwrap(), day).unwrap()
    }

    fn at(year: i32, month: u8, day: u8, hour: u8, minute: u8) -> PrimitiveDateTime {
        PrimitiveDateTime::new(
            ymd(year, month, day),
            Time::from_hms(hour, minute, 0).unwrap(),
        )
    }

    fn event(start: PrimitiveDateTime, recurrence: Recurrence) -> CalendarEvent {
        CalendarEvent {
            id: "e1".into(),
            source: LOCAL_SOURCE.into(),
            title: "Standup".into(),
            notes: String::new(),
            start: LocalDateTime(start),
            all_day: false,
            duration_minutes: None,
            recurrence,
            remind_before_minutes: None,
            created_ms: 0,
            updated_ms: 0,
        }
    }

    /// The starts of the occurrences in [from, to), as "MM-DD HH:MM" for short assertions.
    fn starts(
        event: &CalendarEvent,
        from: PrimitiveDateTime,
        to: PrimitiveDateTime,
    ) -> Vec<String> {
        occurrence_starts(event, from, to)
            .into_iter()
            .map(|s| {
                format!(
                    "{:02}-{:02} {:02}:{:02}",
                    s.month() as u8,
                    s.day(),
                    s.hour(),
                    s.minute()
                )
            })
            .collect()
    }

    #[test]
    fn local_date_times_round_trip_as_text() {
        let text = "2026-10-23T14:05";
        let parsed = LocalDateTime::parse(text).unwrap();
        assert_eq!(parsed.to_string(), text);
        assert_eq!(parsed.0, datetime!(2026-10-23 14:05));
    }

    #[test]
    fn local_date_times_reject_what_is_not_a_real_moment() {
        assert_eq!(LocalDateTime::parse("2026-02-30T10:00"), None);
        assert_eq!(LocalDateTime::parse("2027-02-29T10:00"), None);
        assert_eq!(LocalDateTime::parse("2026-10-23T24:00"), None);
        assert_eq!(LocalDateTime::parse("2026-10-23 10:00"), None);
        assert_eq!(LocalDateTime::parse("2026-10-23"), None);
        assert!(LocalDateTime::parse("2028-02-29T10:00").is_some());
    }

    #[test]
    fn a_huge_duration_does_not_overflow_date_times() {
        for start in [at(-9999, 1, 1, 0, 0), at(9999, 12, 30, 0, 0)] {
            let mut event = event(start, Recurrence::None);
            event.duration_minutes = Some(u32::MAX);
            let to = start + Duration::days(1);
            assert_eq!(occurrence_starts(&event, start, to), vec![start]);
            let expanded = instances(&event, false, start, to);
            assert_eq!(expanded.len(), 1);
            assert_eq!(expanded[0].start.0, start);
            assert_eq!(
                expanded[0].end.0,
                start.checked_add(span(&event)).unwrap_or(start)
            );
            // Also check an occurrence before the window when its end could overflow.
            occurrence_starts(&event, start + Duration::hours(1), to);
        }
    }

    #[test]
    fn a_daily_series_repeats_every_day_at_its_time() {
        let standup = event(at(2026, 10, 1, 9, 30), Recurrence::Daily);
        assert_eq!(
            starts(&standup, at(2026, 10, 3, 0, 0), at(2026, 10, 6, 0, 0)),
            ["10-03 09:30", "10-04 09:30", "10-05 09:30"]
        );
    }

    #[test]
    fn a_weekly_series_keeps_its_weekday() {
        // 2026-10-05 is a Monday, so every date in this series is a Monday.
        let weekly = event(at(2026, 10, 5, 8, 0), Recurrence::Weekly);
        assert_eq!(
            starts(&weekly, at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0)),
            ["10-05 08:00", "10-12 08:00", "10-19 08:00", "10-26 08:00"]
        );
    }

    #[test]
    fn a_monthly_series_on_the_31st_takes_the_last_day_of_short_months() {
        let rent = event(at(2026, 1, 31, 12, 0), Recurrence::Monthly);
        assert_eq!(
            starts(&rent, at(2026, 1, 1, 0, 0), at(2026, 6, 1, 0, 0)),
            [
                "01-31 12:00",
                "02-28 12:00",
                "03-31 12:00",
                "04-30 12:00",
                "05-31 12:00"
            ]
        );
    }

    #[test]
    fn a_monthly_series_goes_back_to_the_31st_after_a_short_month() {
        let rent = event(at(2026, 1, 31, 12, 0), Recurrence::Monthly);
        assert_eq!(
            occurrence_start(rent.start.0, Recurrence::Monthly, 2),
            Some(at(2026, 3, 31, 12, 0))
        );
    }

    #[test]
    fn a_yearly_series_on_29_february_takes_28_february_in_common_years() {
        let birthday = event(at(2028, 2, 29, 9, 0), Recurrence::Yearly);
        assert_eq!(
            occurrence_start(birthday.start.0, Recurrence::Yearly, 1),
            Some(at(2029, 2, 28, 9, 0))
        );
        assert_eq!(
            occurrence_start(birthday.start.0, Recurrence::Yearly, 4),
            Some(at(2032, 2, 29, 9, 0))
        );
        assert_eq!(
            starts(&birthday, at(2029, 1, 1, 0, 0), at(2033, 1, 1, 0, 0)),
            ["02-28 09:00", "02-28 09:00", "02-28 09:00", "02-29 09:00"]
        );
    }

    #[test]
    fn a_one_off_event_appears_once_and_only_in_its_window() {
        let dentist = event(at(2026, 10, 23, 15, 0), Recurrence::None);
        assert_eq!(
            starts(&dentist, at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0)),
            ["10-23 15:00"]
        );
        assert!(starts(&dentist, at(2026, 10, 24, 0, 0), at(2026, 11, 1, 0, 0)).is_empty());
        // The window end is excluded, so the event that starts at it is not in this window.
        assert!(starts(&dentist, at(2026, 10, 1, 0, 0), at(2026, 10, 23, 15, 0)).is_empty());
    }

    #[test]
    fn an_event_that_is_still_running_at_the_window_start_is_included() {
        let mut film = event(at(2026, 10, 22, 23, 0), Recurrence::None);
        film.duration_minutes = Some(180);
        // Runs 23:00 to 02:00, so it overlaps 01:00 on the 23rd.
        assert_eq!(
            starts(&film, at(2026, 10, 23, 1, 0), at(2026, 10, 23, 12, 0)),
            ["10-22 23:00"]
        );
        // It ends at 02:00, so it does not overlap a window that starts at 02:00.
        assert!(starts(&film, at(2026, 10, 23, 2, 0), at(2026, 10, 23, 12, 0)).is_empty());
    }

    #[test]
    fn a_recurring_event_that_runs_across_midnight_is_found_from_the_day_before() {
        let mut night = event(at(2026, 10, 1, 23, 0), Recurrence::Daily);
        night.duration_minutes = Some(180);
        assert_eq!(
            starts(&night, at(2026, 10, 23, 1, 0), at(2026, 10, 23, 12, 0)),
            ["10-22 23:00"]
        );
    }

    #[test]
    fn an_all_day_event_covers_its_whole_day() {
        let mut holiday = event(at(2026, 10, 23, 0, 0), Recurrence::None);
        holiday.all_day = true;
        // Included from the middle of the day, and not once the day is over.
        assert_eq!(
            starts(&holiday, at(2026, 10, 23, 10, 0), at(2026, 10, 24, 10, 0)),
            ["10-23 00:00"]
        );
        assert!(starts(&holiday, at(2026, 10, 24, 0, 0), at(2026, 10, 25, 0, 0)).is_empty());
        let instance = instances(
            &holiday,
            false,
            at(2026, 10, 23, 10, 0),
            at(2026, 10, 24, 0, 0),
        );
        assert_eq!(instance[0].end, LocalDateTime(at(2026, 10, 24, 0, 0)));
        assert!(instance[0].all_day);
    }

    #[test]
    fn check_trims_the_title_and_refuses_an_empty_one() {
        let input = EventInput {
            id: None,
            title: "   ".into(),
            notes: String::new(),
            start: "2026-10-23T10:00".into(),
            all_day: false,
            recurrence: Recurrence::None,
        };
        assert_eq!(check(&input).unwrap_err(), "Give the event a title.");

        let spaced = EventInput {
            title: "  Lunch  ".into(),
            ..input
        };
        assert_eq!(check(&spaced).unwrap().title, "Lunch");
    }

    #[test]
    fn check_refuses_a_bad_start_and_long_text() {
        let base = EventInput {
            id: None,
            title: "Lunch".into(),
            notes: String::new(),
            start: "2026-02-30T10:00".into(),
            all_day: false,
            recurrence: Recurrence::None,
        };
        assert_eq!(check(&base).unwrap_err(), "Pick a valid date and time.");

        let long_title = EventInput {
            title: "x".repeat(MAX_TITLE_CHARS + 1),
            start: "2026-10-23T10:00".into(),
            ..base.clone()
        };
        assert!(check(&long_title).unwrap_err().contains("title"));

        let long_notes = EventInput {
            notes: "x".repeat(MAX_NOTES_CHARS + 1),
            start: "2026-10-23T10:00".into(),
            ..base
        };
        assert!(check(&long_notes).unwrap_err().contains("notes"));
    }

    #[test]
    fn an_all_day_start_is_pinned_to_midnight() {
        let input = EventInput {
            id: None,
            title: "Holiday".into(),
            notes: String::new(),
            start: "2026-10-23T17:45".into(),
            all_day: true,
            recurrence: Recurrence::None,
        };
        assert_eq!(check(&input).unwrap().start.0, datetime!(2026-10-23 00:00));
    }

    #[test]
    fn new_ids_are_32_hex_digits_and_differ() {
        let a = new_id(1);
        let b = new_id(1);
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn recurrence_and_instances_serialize_as_the_frontend_expects() {
        assert_eq!(
            serde_json::to_string(&Recurrence::Monthly).unwrap(),
            "\"monthly\""
        );
        let instance = instances(
            &event(at(2026, 10, 23, 14, 30), Recurrence::None),
            true,
            at(2026, 10, 23, 0, 0),
            at(2026, 10, 24, 0, 0),
        );
        let json = serde_json::to_string(&instance[0]).unwrap();
        assert!(json.contains("\"eventId\":\"e1\""));
        assert!(json.contains("\"start\":\"2026-10-23T14:30\""));
        assert!(json.contains("\"readOnly\":true"));
        assert!(json.contains("\"allDay\":false"));
    }
}
