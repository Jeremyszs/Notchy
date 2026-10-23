//! Reminder rules. Pure: the scheduler passes the wall clock, the occurrences and the settings in, and
//! gets back the reminders due, the pills to show, and the next time to wake.
//!
//! Each occurrence has a trigger for every lead time and one at its start. An all-day event has one
//! trigger, at the all-day hour on its day. A trigger fires once, and its key is kept (in memory, and
//! in `calendar-reminders.json` across restarts) so a restart never shows it twice. A trigger that is
//! less than `OVERDUE_LIMIT` late still fires, which covers a PC that was asleep or a restart just
//! after the time. Anything later is skipped without a word, since a late reminder is noise.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration as StdDuration, Instant};

use serde::Serialize;
use time::{Duration, PrimitiveDateTime, Time};

use super::model::EventInstance;

/// A trigger this late or later is skipped. Less late than this still fires.
pub const OVERDUE_LIMIT: Duration = Duration::minutes(5);
/// Fired records older than this are dropped. A trigger that old is past `OVERDUE_LIMIT`, so it can
/// never fire again, and the record is not needed.
pub const PRUNE_AFTER: Duration = Duration::days(2);
/// How long a lead-time pill and a start pill stay up. A newer one waits its turn.
pub const LEAD_SHOW_FOR: StdDuration = StdDuration::from_secs(6);
pub const START_SHOW_FOR: StdDuration = StdDuration::from_secs(8);
/// Reminders waiting behind the one on screen. The oldest waiting one is dropped beyond this.
pub const MAX_WAITING: usize = 5;

/// Which kind of reminder a trigger is. Both show the same pill, with a different hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Before the start, at a lead time.
    Lead,
    /// At the start, or the all-day hour.
    Start,
}

/// One moment a reminder is due. `start` is the event start the pill counts down to, and `at` is when
/// the reminder fires. For an all-day event the two are the same moment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trigger {
    pub key: String,
    pub at: PrimitiveDateTime,
    pub start: PrimitiveDateTime,
    pub title: String,
    pub kind: Kind,
}

/// The triggers for these occurrences, in time order. `default_lead` applies to an event with no
/// lead times of its own. `all_day_hour` is the hour (0 to 23) an all-day event is announced at.
pub fn triggers(instances: &[EventInstance], default_lead: u32, all_day_hour: u8) -> Vec<Trigger> {
    let mut out = Vec::new();
    for instance in instances {
        let start = instance.start.0;
        if instance.all_day {
            let hour = Time::from_hms(all_day_hour, 0, 0).unwrap_or(Time::MIDNIGHT);
            let at = PrimitiveDateTime::new(start.date(), hour);
            out.push(Trigger {
                key: format!("{}|{}|day", instance.event_id, instance.start),
                at,
                start: at,
                title: instance.title.clone(),
                kind: Kind::Start,
            });
            continue;
        }
        let leads = instance
            .remind_before_minutes
            .clone()
            .unwrap_or_else(|| vec![default_lead]);
        // A lead of zero is the start itself, so it is not announced twice.
        let offsets: BTreeSet<u32> = leads.into_iter().chain([0]).collect();
        for offset in offsets {
            out.push(Trigger {
                key: format!("{}|{}|{offset}", instance.event_id, instance.start),
                at: start - Duration::minutes(i64::from(offset)),
                start,
                title: instance.title.clone(),
                kind: if offset == 0 { Kind::Start } else { Kind::Lead },
            });
        }
    }
    out.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.key.cmp(&b.key)));
    out
}

/// Whole minutes from `now` to `start`, rounded up. Zero once the start has passed, so the pill reads
/// "now".
pub fn minutes_until(start: PrimitiveDateTime, now: PrimitiveDateTime) -> i64 {
    let seconds = (start - now).whole_seconds();
    if seconds <= 0 { 0 } else { (seconds + 59) / 60 }
}

/// The reminders already shown, and the rule for showing the next ones.
#[derive(Debug, Default)]
pub struct Reminders {
    fired: BTreeMap<String, PrimitiveDateTime>,
    changed: bool,
    last: Option<(Trigger, PrimitiveDateTime)>,
    snoozed: Vec<Trigger>,
    snooze_seq: u64,
}

impl Reminders {
    /// Starts from the records read from the file.
    pub fn from_fired(fired: BTreeMap<String, PrimitiveDateTime>) -> Self {
        Self {
            fired,
            ..Self::default()
        }
    }

    /// The records to write to the file.
    pub fn fired(&self) -> &BTreeMap<String, PrimitiveDateTime> {
        &self.fired
    }

    /// The triggers due at `now`, each recorded as fired so it cannot fire again. A trigger still
    /// ahead of `now` waits. One that is overdue by `OVERDUE_LIMIT` or more is skipped, and not
    /// recorded, because it never becomes due again.
    pub fn due(&mut self, now: PrimitiveDateTime, triggers: &[Trigger]) -> Vec<Trigger> {
        let mut due = Vec::new();
        for trigger in triggers {
            if trigger.at > now || now - trigger.at >= OVERDUE_LIMIT {
                continue;
            }
            if self.fired.contains_key(&trigger.key) {
                continue;
            }
            self.fired.insert(trigger.key.clone(), trigger.at);
            self.changed = true;
            due.push(trigger.clone());
        }
        self.snoozed.retain(|trigger| {
            if trigger.at > now {
                return true;
            }
            if now - trigger.at < OVERDUE_LIMIT {
                due.push(trigger.clone());
            }
            false
        });
        due.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.key.cmp(&b.key)));
        if let Some(trigger) = due.last() {
            self.last = Some((trigger.clone(), now));
        }
        due
    }

    pub fn recent(&self, now: PrimitiveDateTime) -> Option<RecentReminder> {
        let (trigger, fired) = self.last.as_ref()?;
        let remaining = *fired + Duration::minutes(30) - now;
        (remaining > Duration::ZERO).then(|| RecentReminder {
            title: trigger.title.clone(),
            expires_in_ms: u64::try_from(remaining.whole_milliseconds()).unwrap_or(0),
        })
    }

    pub fn expiry(&self, now: PrimitiveDateTime) -> Option<PrimitiveDateTime> {
        self.recent(now)?;
        self.last
            .as_ref()
            .map(|(_, fired)| *fired + Duration::minutes(30))
    }

    pub fn expire(&mut self, now: PrimitiveDateTime) -> bool {
        if self.last.is_some() && self.recent(now).is_none() {
            self.last = None;
            return true;
        }
        false
    }

    pub fn snooze(&mut self, minutes: u32, now: PrimitiveDateTime) -> bool {
        if ![5, 10].contains(&minutes) || self.recent(now).is_none() {
            return false;
        }
        let Some(at) = now.checked_add(Duration::minutes(i64::from(minutes))) else {
            return false;
        };
        let (mut trigger, _) = self.last.take().unwrap();
        self.snooze_seq += 1;
        trigger.key = format!("{}|snooze|{}", trigger.key, self.snooze_seq);
        trigger.at = at;
        trigger.start = at;
        trigger.kind = Kind::Start;
        self.snoozed.push(trigger);
        true
    }

    /// The moment of the next trigger that is still ahead and has not fired, or None.
    pub fn next(&self, now: PrimitiveDateTime, triggers: &[Trigger]) -> Option<PrimitiveDateTime> {
        triggers
            .iter()
            .chain(self.snoozed.iter())
            .filter(|trigger| trigger.at > now && !self.fired.contains_key(&trigger.key))
            .map(|trigger| trigger.at)
            .min()
    }

    /// Drops records too old to matter. Returns nothing: `take_changed` reports the change.
    pub fn prune(&mut self, now: PrimitiveDateTime) {
        let before = self.fired.len();
        self.fired.retain(|_, at| now - *at < PRUNE_AFTER);
        self.changed |= self.fired.len() != before;
    }

    /// Whether the records moved since the last call, so the file is written only when it must be.
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentReminder {
    pub title: String,
    pub expires_in_ms: u64,
}

/// One reminder on screen. Mirrors `CalendarNotice` in src/lib/calendar.ts. `seq` counts up from one,
/// so the island can tell a new one from the one before.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub seq: u64,
    pub title: String,
    pub minutes_until: i64,
}

/// What the island should do after a queue step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Show this reminder now. Its payload goes out before the island state changes.
    Show(Notice),
    /// Nothing is left to show. The island gives the calendar state up.
    Hide,
}

/// The reminder on screen and the ones waiting. One shows at a time, each for its hold. A newer one
/// waits its turn rather than cutting the current one short. Every method takes the instant to act
/// at, so the tests never read the clock.
#[derive(Debug, Default)]
pub struct Queue {
    next_seq: u64,
    waiting: VecDeque<(Notice, StdDuration)>,
    /// The reminder on screen, and when it gives way.
    showing: Option<(Notice, Instant)>,
}

impl Queue {
    /// Adds a reminder. It shows at once when nothing is up. Otherwise it waits, and beyond
    /// `MAX_WAITING` the oldest waiting one is dropped.
    pub fn push(&mut self, trigger: &Trigger, minutes_until: i64, now: Instant) -> Vec<Step> {
        self.next_seq += 1;
        let notice = Notice {
            seq: self.next_seq,
            title: trigger.title.clone(),
            minutes_until,
        };
        let hold = match trigger.kind {
            Kind::Lead => LEAD_SHOW_FOR,
            Kind::Start => START_SHOW_FOR,
        };
        if self.showing.is_some() {
            self.waiting.push_back((notice, hold));
            while self.waiting.len() > MAX_WAITING {
                self.waiting.pop_front();
            }
            Vec::new()
        } else {
            self.show(notice, hold, now)
        }
    }

    /// Gives way once the one on screen has had its time. The next waiting one shows, or the island
    /// hides when none is left.
    pub fn advance(&mut self, now: Instant) -> Vec<Step> {
        match &self.showing {
            Some((_, until)) if now >= *until => {}
            _ => return Vec::new(),
        }
        self.showing = None;
        match self.waiting.pop_front() {
            Some((next, hold)) => self.show(next, hold, now),
            None => vec![Step::Hide],
        }
    }

    /// Drops everything, for when the calendar is turned off. Hides the one on screen.
    pub fn clear(&mut self) -> Vec<Step> {
        self.waiting.clear();
        match self.showing.take() {
            Some(_) => vec![Step::Hide],
            None => Vec::new(),
        }
    }

    /// When the reminder on screen gives way, or None when nothing is on screen.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.showing.as_ref().map(|(_, until)| *until)
    }

    fn show(&mut self, notice: Notice, hold: StdDuration, now: Instant) -> Vec<Step> {
        self.showing = Some((notice.clone(), now + hold));
        vec![Step::Show(notice)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::calendar::model::{CalendarEvent, Recurrence};
    use crate::modules::calendar::model::{LocalDateTime, instances};
    use time::macros::datetime;

    fn instance(id: &str, start: PrimitiveDateTime, all_day: bool) -> EventInstance {
        EventInstance {
            event_id: id.into(),
            source: "local".into(),
            title: format!("Event {id}"),
            notes: String::new(),
            start: LocalDateTime(start),
            end: LocalDateTime(start),
            all_day,
            read_only: false,
            remind_before_minutes: None,
        }
    }

    fn keys(triggers: &[Trigger]) -> Vec<String> {
        triggers.iter().map(|t| t.key.clone()).collect()
    }

    #[test]
    fn snoozes_fire_once_after_five_or_ten_minutes_and_survive_prune() {
        for minutes in [5, 10] {
            let now = datetime!(2026-10-23 14:30);
            let original = triggers(&[instance("a", now, false)], 10, 9);
            let mut reminders = Reminders::default();
            assert_eq!(reminders.due(now, &original).len(), 1);
            let fired = reminders.fired().clone();
            assert!(reminders.snooze(minutes, now));
            assert_eq!(reminders.recent(now), None);
            assert!(!reminders.snooze(minutes, now));
            let at = now + Duration::minutes(i64::from(minutes));
            assert_eq!(reminders.next(now, &[]), Some(at));
            reminders.prune(now);
            assert!(reminders.due(at - Duration::seconds(1), &[]).is_empty());
            let due = reminders.due(at, &[]);
            assert_eq!(due.len(), 1);
            assert!(due[0].key.contains("|snooze|"));
            assert_eq!(due[0].title, "Event a");
            assert_eq!(due[0].kind, Kind::Start);
            assert_eq!(minutes_until(due[0].start, at), 0);
            assert!(reminders.due(at, &[]).is_empty());
            assert_eq!(reminders.fired(), &fired, "snoozes are never persisted");
            assert_eq!(reminders.recent(at).unwrap().expires_in_ms, 30 * 60_000);
        }
    }

    #[test]
    fn snooze_requires_allowed_minutes_and_a_recent_trigger() {
        let now = datetime!(2026-10-23 14:30);
        let mut reminders = Reminders::default();
        assert!(!reminders.snooze(5, now));
        reminders.due(now, &triggers(&[instance("a", now, false)], 10, 9));
        for minutes in [0, 1, 6, 30, u32::MAX] {
            assert!(!reminders.snooze(minutes, now));
        }
        assert_eq!(reminders.expiry(now), Some(now + Duration::minutes(30)));
        assert!(
            reminders
                .recent(now + Duration::minutes(30) - Duration::milliseconds(1))
                .is_some()
        );
        let expiry = now + Duration::minutes(30);
        assert_eq!(reminders.recent(expiry), None);
        assert!(!reminders.snooze(5, expiry));
        assert!(reminders.expire(expiry));
        assert!(!reminders.expire(expiry));
        assert_eq!(reminders.expiry(expiry), None);
    }

    #[test]
    fn the_newest_due_reminder_replaces_the_recent_one() {
        let now = datetime!(2026-10-23 14:30);
        let mut reminders = Reminders::default();
        reminders.due(now, &triggers(&[instance("a", now, false)], 10, 9));
        let later = now + Duration::minutes(1);
        reminders.due(later, &triggers(&[instance("b", later, false)], 10, 9));
        assert_eq!(reminders.recent(later).unwrap().title, "Event b");
        assert_eq!(
            serde_json::to_value(reminders.recent(later)).unwrap(),
            serde_json::json!({"title": "Event b", "expiresInMs": 1_800_000})
        );
        assert!(reminders.snooze(5, later));
        assert_eq!(
            reminders.due(later + Duration::minutes(5), &[])[0].title,
            "Event b"
        );
    }

    #[test]
    fn a_snoozed_reminder_uses_the_same_queue_and_overdue_limit() {
        let wall = datetime!(2026-10-23 14:30);
        let instant = Instant::now();
        let mut reminders = Reminders::default();
        let trigger = reminders
            .due(wall, &triggers(&[instance("a", wall, false)], 10, 9))
            .remove(0);
        let mut queue = Queue::default();
        assert_eq!(queue.push(&trigger, 0, instant).len(), 1);
        assert!(reminders.snooze(5, wall));
        let at = wall + Duration::minutes(5);
        let snoozed = reminders.due(at, &[]).remove(0);
        assert!(queue.push(&snoozed, 0, instant).is_empty());
        let steps = queue.advance(instant + START_SHOW_FOR);
        assert!(
            matches!(&steps[..], [Step::Show(notice)] if notice.title == "Event a" && notice.minutes_until == 0)
        );
        assert!(reminders.snooze(10, at));
        assert!(reminders.due(at + Duration::minutes(15), &[]).is_empty());
        assert_eq!(reminders.next(at + Duration::minutes(15), &[]), None);
    }

    #[test]
    fn a_timed_event_has_a_lead_trigger_and_a_start_trigger() {
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].at, datetime!(2026-10-23 14:20));
        assert_eq!(found[0].kind, Kind::Lead);
        assert_eq!(found[1].at, datetime!(2026-10-23 14:30));
        assert_eq!(found[1].kind, Kind::Start);
    }

    #[test]
    fn an_event_can_carry_its_own_lead_times() {
        let mut own = instance("a", datetime!(2026-10-23 14:30), false);
        own.remind_before_minutes = Some(vec![30, 5, 0]);
        let found = triggers(&[own], 10, 9);
        // Zero is the start, so it is not counted twice.
        let at: Vec<_> = found.iter().map(|t| t.at).collect();
        assert_eq!(
            at,
            [
                datetime!(2026-10-23 14:00),
                datetime!(2026-10-23 14:25),
                datetime!(2026-10-23 14:30),
            ]
        );
    }

    #[test]
    fn an_all_day_event_has_one_trigger_at_the_all_day_hour() {
        let found = triggers(&[instance("h", datetime!(2026-10-23 00:00), true)], 10, 8);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].at, datetime!(2026-10-23 08:00));
        assert_eq!(found[0].kind, Kind::Start);
    }

    #[test]
    fn a_trigger_fires_once_when_its_time_comes() {
        let mut reminders = Reminders::default();
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        assert!(
            reminders
                .due(datetime!(2026-10-23 14:19), &found)
                .is_empty()
        );
        assert_eq!(reminders.due(datetime!(2026-10-23 14:20), &found).len(), 1);
        assert!(
            reminders
                .due(datetime!(2026-10-23 14:21), &found)
                .is_empty()
        );
        assert_eq!(reminders.due(datetime!(2026-10-23 14:30), &found).len(), 1);
        assert!(
            reminders
                .due(datetime!(2026-10-23 14:31), &found)
                .is_empty()
        );
    }

    #[test]
    fn an_overdue_trigger_under_five_minutes_still_fires() {
        let mut reminders = Reminders::default();
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        // The lead trigger was due at 14:20. At 14:24 it is four minutes late, so it still shows.
        let due = reminders.due(datetime!(2026-10-23 14:24), &found);
        assert_eq!(keys(&due), vec![found[0].key.clone()]);
    }

    #[test]
    fn an_overdue_trigger_of_five_minutes_or_more_is_skipped() {
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        // The lead trigger was due at 14:20. At 14:25 it is exactly five minutes late, so it is skipped.
        let mut late = Reminders::default();
        let due = late.due(datetime!(2026-10-23 14:25), &found[..1]);
        assert!(
            due.is_empty(),
            "a lead reminder five minutes late is skipped"
        );
        assert!(
            late.fired().is_empty(),
            "a skipped reminder is not recorded"
        );
    }

    #[test]
    fn a_restart_does_not_show_a_reminder_again() {
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        let mut before = Reminders::default();
        assert_eq!(before.due(datetime!(2026-10-23 14:30), &found).len(), 1);
        assert!(before.take_changed());

        // The record is written and read back, as after a restart at 14:31.
        let mut after = Reminders::from_fired(before.fired().clone());
        assert!(after.due(datetime!(2026-10-23 14:31), &found).is_empty());
        assert!(!after.take_changed());
    }

    #[test]
    fn a_restart_just_after_a_start_still_shows_it_once() {
        // The app was off at 14:20, so the lead trigger is still due on start-up, and shows once.
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        let mut reminders = Reminders::default();
        assert_eq!(reminders.due(datetime!(2026-10-23 14:21), &found).len(), 1);
        assert!(
            reminders
                .due(datetime!(2026-10-23 14:22), &found)
                .is_empty()
        );
    }

    #[test]
    fn the_next_wake_is_the_soonest_trigger_still_ahead() {
        let reminders = Reminders::default();
        let found = triggers(&[instance("a", datetime!(2026-10-23 14:30), false)], 10, 9);
        assert_eq!(
            reminders.next(datetime!(2026-10-23 14:00), &found),
            Some(datetime!(2026-10-23 14:20))
        );
        assert_eq!(
            reminders.next(datetime!(2026-10-23 14:25), &found),
            Some(datetime!(2026-10-23 14:30))
        );
        assert_eq!(reminders.next(datetime!(2026-10-23 15:00), &found), None);
    }

    #[test]
    fn records_older_than_two_days_are_pruned() {
        let mut reminders = Reminders::from_fired(BTreeMap::from([
            ("old".to_string(), datetime!(2026-10-20 10:00)),
            ("recent".to_string(), datetime!(2026-10-22 10:00)),
        ]));
        reminders.prune(datetime!(2026-10-23 10:00));
        assert_eq!(reminders.fired().keys().collect::<Vec<_>>(), ["recent"]);
        assert!(reminders.take_changed());
    }

    #[test]
    fn a_weekly_series_gives_each_occurrence_its_own_reminder() {
        let weekly = CalendarEvent {
            id: "w".into(),
            source: "local".into(),
            title: "Standup".into(),
            notes: String::new(),
            start: LocalDateTime(datetime!(2026-10-23 09:00)),
            all_day: false,
            duration_minutes: None,
            recurrence: Recurrence::Weekly,
            remind_before_minutes: None,
            created_ms: 0,
            updated_ms: 0,
        };
        let found = instances(
            &weekly,
            false,
            datetime!(2026-10-23 00:00),
            datetime!(2026-11-06 00:00),
        );
        assert_eq!(
            found.len(),
            2,
            "the 23rd and the 30th; the 6th is outside the window"
        );
        let mut reminders = Reminders::default();
        // At 08:52 on the 30th the 08:50 lead is two minutes late, so it shows. The 23rd is long past.
        let due = reminders.due(datetime!(2026-10-30 08:52), &triggers(&found, 10, 9));
        assert_eq!(due.len(), 1);
        assert!(due[0].key.contains("2026-10-30T09:00|10"), "{}", due[0].key);
    }

    #[test]
    fn minutes_until_rounds_up_and_reads_zero_once_started() {
        let start = datetime!(2026-10-23 14:30);
        assert_eq!(minutes_until(start, datetime!(2026-10-23 14:20)), 10);
        assert_eq!(minutes_until(start, datetime!(2026-10-23 14:29)), 1);
        assert_eq!(minutes_until(start, datetime!(2026-10-23 14:29:30)), 1);
        assert_eq!(minutes_until(start, datetime!(2026-10-23 14:30)), 0);
        assert_eq!(minutes_until(start, datetime!(2026-10-23 14:45)), 0);
    }

    fn pending(title: &str, kind: Kind) -> Trigger {
        Trigger {
            key: title.into(),
            at: datetime!(2026-10-23 14:20),
            start: datetime!(2026-10-23 14:30),
            title: title.into(),
            kind,
        }
    }

    #[test]
    fn the_first_reminder_shows_at_once_and_the_next_waits_its_turn() {
        let start = Instant::now();
        let mut queue = Queue::default();
        let first = queue.push(&pending("Standup", Kind::Lead), 10, start);
        assert_eq!(
            first,
            vec![Step::Show(Notice {
                seq: 1,
                title: "Standup".into(),
                minutes_until: 10
            })]
        );
        assert!(
            queue
                .push(&pending("Lunch", Kind::Start), 0, start)
                .is_empty()
        );
        assert_eq!(queue.next_deadline(), Some(start + LEAD_SHOW_FOR));
    }

    #[test]
    fn a_start_pill_holds_longer_than_a_lead_pill() {
        let start = Instant::now();
        let mut queue = Queue::default();
        queue.push(&pending("Standup", Kind::Start), 0, start);
        assert_eq!(queue.next_deadline(), Some(start + START_SHOW_FOR));
    }

    #[test]
    fn the_queue_moves_on_in_order_and_hides_when_empty() {
        let start = Instant::now();
        let mut queue = Queue::default();
        queue.push(&pending("A", Kind::Lead), 10, start);
        queue.push(&pending("B", Kind::Lead), 9, start);
        let next = queue.advance(start + LEAD_SHOW_FOR);
        assert_eq!(
            next,
            vec![Step::Show(Notice {
                seq: 2,
                title: "B".into(),
                minutes_until: 9
            })]
        );
        assert_eq!(queue.advance(start + LEAD_SHOW_FOR * 2), vec![Step::Hide]);
        assert!(queue.advance(start + LEAD_SHOW_FOR * 3).is_empty());
    }

    #[test]
    fn the_queue_keeps_only_the_newest_waiting_reminders() {
        let start = Instant::now();
        let mut queue = Queue::default();
        queue.push(&pending("first", Kind::Lead), 10, start);
        for n in 0..(MAX_WAITING + 2) {
            queue.push(&pending(&format!("w{n}"), Kind::Lead), 10, start);
        }
        let mut titles = Vec::new();
        let mut now = start;
        loop {
            now += START_SHOW_FOR;
            match queue.advance(now).into_iter().next() {
                Some(Step::Show(notice)) => titles.push(notice.title),
                Some(Step::Hide) | None => break,
            }
        }
        assert_eq!(titles, ["w2", "w3", "w4", "w5", "w6"]);
    }

    #[test]
    fn clearing_drops_the_queue_and_hides_what_is_on_screen() {
        let start = Instant::now();
        let mut queue = Queue::default();
        queue.push(&pending("A", Kind::Lead), 10, start);
        queue.push(&pending("B", Kind::Lead), 10, start);
        assert_eq!(queue.clear(), vec![Step::Hide]);
        assert!(queue.advance(start + LEAD_SHOW_FOR * 2).is_empty());
        assert!(queue.clear().is_empty());
    }

    #[test]
    fn the_notice_serializes_for_the_island() {
        let notice = Notice {
            seq: 3,
            title: "Standup".into(),
            minutes_until: 10,
        };
        assert_eq!(
            serde_json::to_string(&notice).unwrap(),
            r#"{"seq":3,"title":"Standup","minutesUntil":10}"#
        );
    }
}
