//! Synced lyrics from LRCLIB, keyed by the track the media module reports.
//!
//! The media listener only parses the update and queues work. Each new track gets a worker
//! thread for its HTTP requests, so a slow network never blocks media events. Results are
//! cached in memory, and a response for a track that has since changed is cached but never
//! emitted.
//!
//! Privacy: the cleaned title and artist, plus the album and duration when known, are sent
//! to lrclib.net for every track that is not already cached. With lyrics off in the settings,
//! nothing is sent and the panel shows no lyrics.

use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Listener, Manager};

use super::media::MediaUpdate;
use super::{Module, ModuleContext};
use crate::config;

const MEDIA_EVENT: &str = "media://update";
const UPDATE_EVENT: &str = "lyrics://update";
const API: &str = "https://lrclib.net/api";
const USER_AGENT: &str = "Notchy/0.1 (https://github.com/notchy)";
const TIMEOUT: Duration = Duration::from_secs(6);
/// A search hit is only used when its length is within this many seconds of the track.
const DURATION_TOLERANCE_S: f64 = 5.0;
const CACHE_SIZE: usize = 20;

/// The frontend contract. Mirrors LyricsState in src/stores/lyrics.ts.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsUpdate {
    pub key: String,
    pub status: Status,
    pub lines: Vec<Line>,
    pub plain: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Loading,
    Synced,
    Plain,
    #[default]
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub time_ms: u64,
    pub text: String,
}

impl LyricsUpdate {
    fn none(key: &str) -> Self {
        Self {
            key: key.to_owned(),
            ..Self::default()
        }
    }
}

/// Shared with the Tauri commands. The mutex guards everything, so an update and its
/// emit always happen in the same order.
#[derive(Default)]
struct LyricsHub {
    state: Mutex<LyricsState>,
}

#[derive(Default)]
struct LyricsState {
    /// Bumped on every change of track, so a finished fetch can tell it is stale.
    generation: u64,
    /// The payload last emitted. Its key names the track it belongs to.
    latest: LyricsUpdate,
    /// The track the media module reported last, kept so a settings change can re-run it.
    query: Option<Query>,
    /// Finished results, least recently used first. Negative results are kept too.
    cache: Vec<LyricsUpdate>,
}

impl LyricsState {
    fn cached(&mut self, key: &str) -> Option<LyricsUpdate> {
        let hit = self.cache.iter().find(|entry| entry.key == key)?.clone();
        self.remember(hit.clone());
        Some(hit)
    }

    fn remember(&mut self, update: LyricsUpdate) {
        self.cache.retain(|entry| entry.key != update.key);
        self.cache.push(update);
        if self.cache.len() > CACHE_SIZE {
            self.cache.remove(0);
        }
    }
}

pub struct LyricsModule;

impl Module for LyricsModule {
    fn name(&self) -> &'static str {
        "lyrics"
    }

    fn start(&self, ctx: ModuleContext) {
        let hub = Arc::new(LyricsHub::default());
        ctx.app.manage(hub.clone());

        let app = ctx.app.clone();
        ctx.app.listen(MEDIA_EVENT, move |event| {
            // The payload is the JSON the media module emitted.
            let Ok(update) = serde_json::from_str::<MediaUpdate>(event.payload()) else {
                return;
            };
            request(&app, &hub, Query::from_media(&update));
        });
    }
}

/// What LRCLIB is asked for, built from the cleaned title and artist.
#[derive(Debug, Clone, PartialEq)]
struct Query {
    key: String,
    title: String,
    artist: String,
    album: String,
    duration_s: Option<u64>,
}

impl Query {
    /// None when there is nothing to look up: media is not active, or it has no title.
    fn from_media(update: &MediaUpdate) -> Option<Self> {
        if !update.active {
            return None;
        }
        let (title, artist) = clean_track(&update.title, &update.artist);
        if title.is_empty() {
            return None;
        }
        Some(Self {
            key: track_key(&title, &artist),
            title,
            artist,
            album: update.album.trim().to_owned(),
            duration_s: (update.duration_ms > 0).then(|| (update.duration_ms + 500) / 1000),
        })
    }
}

/// Runs on the listener thread, so it only touches memory and starts the fetch.
fn request(app: &AppHandle, hub: &Arc<LyricsHub>, query: Option<Query>) {
    let enabled = config::lyrics_enabled(app);
    let mut state = hub.state.lock().unwrap_or_else(PoisonError::into_inner);
    state.query = query.clone();
    let key = query.as_ref().map(|q| q.key.clone()).unwrap_or_default();
    if key == state.latest.key {
        return;
    }
    publish(app, hub, &mut state, query, enabled);
}

/// Called when the lyrics setting changes. Runs the current track again under the new setting,
/// so turning lyrics on fetches it now, and turning them off clears the panel.
pub fn reapply(app: &AppHandle) {
    let Some(hub) = app
        .try_state::<Arc<LyricsHub>>()
        .map(|hub| hub.inner().clone())
    else {
        return;
    };
    let enabled = config::lyrics_enabled(app);
    let mut state = hub.state.lock().unwrap_or_else(PoisonError::into_inner);
    let query = state.query.clone();
    publish(app, &hub, &mut state, query, enabled);
}

/// Makes `query` the current track and emits its state. The caller holds the hub lock.
/// With lyrics off the track shows as "none" and nothing is fetched or looked up in the cache.
fn publish(
    app: &AppHandle,
    hub: &Arc<LyricsHub>,
    state: &mut LyricsState,
    query: Option<Query>,
    enabled: bool,
) {
    state.generation += 1;
    let next = match query {
        None => LyricsUpdate::default(),
        Some(query) if !enabled => LyricsUpdate::none(&query.key),
        Some(query) => match state.cached(&query.key) {
            Some(hit) => hit,
            None => {
                spawn_fetch(app.clone(), hub.clone(), state.generation, query.clone());
                LyricsUpdate {
                    key: query.key,
                    status: Status::Loading,
                    ..LyricsUpdate::default()
                }
            }
        },
    };
    state.latest = next;
    let _ = app.emit(UPDATE_EVENT, &state.latest);
}

fn spawn_fetch(app: AppHandle, hub: Arc<LyricsHub>, generation: u64, query: Query) {
    thread::spawn(move || {
        let fetched = fetch(&query);
        // A failed request shows as no lyrics, but is not cached, so the next play retries.
        let cacheable = fetched.is_ok();
        let update = match fetched {
            Ok(record) => to_update(&query.key, record),
            Err(_) => LyricsUpdate::none(&query.key),
        };

        let mut state = hub.state.lock().unwrap_or_else(PoisonError::into_inner);
        if cacheable {
            state.remember(update.clone());
        }
        if state.generation != generation {
            return;
        }
        // Lyrics may have been switched off while the request was in flight. The result is
        // still cached above, so turning them back on later does not need the request again.
        state.latest = if config::lyrics_enabled(&app) {
            update
        } else {
            LyricsUpdate::none(&query.key)
        };
        let _ = app.emit(UPDATE_EVENT, &state.latest);
    });
}

/// The fields of an LRCLIB record that matter here.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    duration: Option<f64>,
}

impl Record {
    fn has_lyrics(&self) -> bool {
        has_text(self.synced_lyrics.as_deref()) || has_text(self.plain_lyrics.as_deref())
    }
}

fn has_text(text: Option<&str>) -> bool {
    text.is_some_and(|text| !text.trim().is_empty())
}

/// Exact match first, then search. `Ok(None)` means LRCLIB has no usable lyrics for the track.
/// `Err` means the request itself failed, which callers treat differently.
fn fetch(query: &Query) -> Result<Option<Record>, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(USER_AGENT)
        .build()
        .into();

    // The exact endpoint needs an artist, so a track without one goes straight to search.
    if !query.artist.is_empty() {
        let mut exact = agent
            .get(format!("{API}/get"))
            .query("track_name", &query.title)
            .query("artist_name", &query.artist);
        if !query.album.is_empty() {
            exact = exact.query("album_name", &query.album);
        }
        if let Some(duration) = query.duration_s {
            exact = exact.query("duration", duration.to_string());
        }
        match exact.call() {
            Ok(mut response) => {
                let body = response.body_mut().read_to_string()?;
                return Ok(serde_json::from_str(&body).ok());
            }
            Err(ureq::Error::StatusCode(404)) => {}
            Err(err) => return Err(err),
        }
    }

    let mut search = agent
        .get(format!("{API}/search"))
        .query("track_name", &query.title);
    if !query.artist.is_empty() {
        search = search.query("artist_name", &query.artist);
    }
    let mut response = search.call()?;
    let body = response.body_mut().read_to_string()?;
    let results: Vec<Record> = serde_json::from_str(&body).unwrap_or_default();
    Ok(pick_best(results, query.duration_s))
}

/// Chooses among search hits. Without a known duration, the first hit with lyrics wins.
/// With one, the closest hit within the tolerance wins, and no hit is used otherwise.
fn pick_best(results: Vec<Record>, duration_s: Option<u64>) -> Option<Record> {
    let mut candidates = results.into_iter().filter(Record::has_lyrics);
    let Some(wanted) = duration_s else {
        return candidates.next();
    };
    let wanted = wanted as f64;
    let gap = |record: &Record| {
        record
            .duration
            .map_or(f64::INFINITY, |duration| (duration - wanted).abs())
    };
    candidates
        .filter(|record| gap(record) <= DURATION_TOLERANCE_S)
        .min_by(|a, b| gap(a).total_cmp(&gap(b)))
}

fn to_update(key: &str, record: Option<Record>) -> LyricsUpdate {
    let Some(record) = record else {
        return LyricsUpdate::none(key);
    };
    let lines = record
        .synced_lyrics
        .as_deref()
        .map(parse_lrc)
        .unwrap_or_default();
    if !lines.is_empty() {
        return LyricsUpdate {
            key: key.to_owned(),
            status: Status::Synced,
            lines,
            plain: None,
        };
    }
    match record
        .plain_lyrics
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
    {
        Some(plain) => LyricsUpdate {
            key: key.to_owned(),
            status: Status::Plain,
            lines: Vec::new(),
            plain: Some(plain),
        },
        None => LyricsUpdate::none(key),
    }
}

/// Parses LRC text into timed lines, sorted by time. Lines without text are dropped, and
/// metadata tags such as `[ar:Artist]` are skipped.
pub fn parse_lrc(text: &str) -> Vec<Line> {
    let mut lines: Vec<Line> = text.lines().flat_map(parse_lrc_line).collect();
    lines.sort_by_key(|line| line.time_ms);
    lines
}

/// A line may carry several leading timestamps, and each one yields its own entry.
fn parse_lrc_line(raw: &str) -> Vec<Line> {
    let mut rest = raw.trim();
    let mut times = Vec::new();
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some(close) = after_open.find(']') else {
            break;
        };
        let Some(time_ms) = parse_timestamp(&after_open[..close]) else {
            break;
        };
        times.push(time_ms);
        rest = &after_open[close + 1..];
    }

    let text = rest.trim();
    if text.is_empty() {
        return Vec::new();
    }
    times
        .into_iter()
        .map(|time_ms| Line {
            time_ms,
            text: text.to_owned(),
        })
        .collect()
}

/// `mm:ss`, `mm:ss.xx` or `mm:ss.xxx`, in milliseconds. Anything else is not a timestamp.
fn parse_timestamp(tag: &str) -> Option<u64> {
    let (minutes, rest) = tag.split_once(':')?;
    let (seconds, fraction) = rest.split_once('.').unwrap_or((rest, ""));
    let millis = if fraction.is_empty() {
        0
    } else {
        // Centiseconds and milliseconds both show up, so scale by how many digits there are.
        let digits = fraction.get(..fraction.len().min(3))?;
        let value = u64::from(number(digits)?);
        value * 10u64.pow((3 - digits.len()) as u32)
    };
    Some(u64::from(number(minutes)?) * 60_000 + u64::from(number(seconds)?) * 1_000 + millis)
}

/// A non-empty run of ASCII digits.
fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// "Song - Live" or "Song - 2011 Remaster" is a version tag on the song, not "Artist - Song".
fn is_version_suffix(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "live", "remaster", "remix", "version", "edit", "acoustic", "mono", "stereo", "demo",
        "radio", "single", "feat",
    ]
    .iter()
    .any(|tag| lower.contains(tag))
}

/// Removes the decoration that video uploads add, and splits "Artist - Song" titles.
/// Returns `(title, artist)`, both trimmed. The title is empty when nothing usable is left.
pub fn clean_track(title: &str, artist: &str) -> (String, String) {
    let title = strip_noise_brackets(title);
    let artist = strip_topic(artist);
    if let Some((left, right)) = title.split_once(" - ") {
        let (left, right) = (left.trim(), right.trim());
        if !left.is_empty() && !right.is_empty() && !is_version_suffix(right) {
            return (right.to_owned(), left.to_owned());
        }
    }
    (title.trim().to_owned(), artist)
}

/// Drops `(...)` and `[...]` groups whose words say they are video decoration. Other groups,
/// such as `(Remix)`, stay.
fn strip_noise_brackets(title: &str) -> String {
    let mut out = String::new();
    let mut rest = title;
    while let Some(open) = rest.find(['(', '[']) {
        let close_char = if rest.as_bytes()[open] == b'(' {
            ')'
        } else {
            ']'
        };
        let Some(close_offset) = rest[open + 1..].find(close_char) else {
            break;
        };
        let close = open + 1 + close_offset;
        out.push_str(&rest[..open]);
        if !is_noise(&rest[open + 1..close]) {
            out.push_str(&rest[open..=close]);
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_noise(inner: &str) -> bool {
    inner
        .split(|c: char| !c.is_alphanumeric() && c != '.')
        .any(|word| {
            matches!(
                word.to_ascii_lowercase().as_str(),
                "official"
                    | "lyric"
                    | "lyrics"
                    | "audio"
                    | "visualizer"
                    | "video"
                    | "hd"
                    | "hq"
                    | "4k"
                    | "ft"
                    | "ft."
                    | "feat"
                    | "feat."
                    | "featuring"
            )
        })
}

/// YouTube auto-generated artist channels end in " - Topic".
fn strip_topic(artist: &str) -> String {
    let artist = artist.trim();
    let lower = artist.to_ascii_lowercase();
    match lower.strip_suffix(" - topic") {
        Some(stem) => artist[..stem.len()].trim_end().to_owned(),
        None => artist.to_owned(),
    }
}

/// Lowercased, whitespace-collapsed `title|artist`. It is the cache key and the frontend's
/// identity for a track.
fn track_key(title: &str, artist: &str) -> String {
    format!("{}|{}", normalize(title), normalize(artist))
}

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[tauri::command]
pub fn lyrics_snapshot(app: AppHandle) -> LyricsUpdate {
    app.try_state::<Arc<LyricsHub>>()
        .map(|hub| {
            hub.state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .latest
                .clone()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(synced: Option<&str>, plain: Option<&str>, duration: Option<f64>) -> Record {
        Record {
            synced_lyrics: synced.map(str::to_owned),
            plain_lyrics: plain.map(str::to_owned),
            duration,
        }
    }

    fn line(time_ms: u64, text: &str) -> Line {
        Line {
            time_ms,
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_centiseconds_and_milliseconds() {
        let lines = parse_lrc("[00:01.50] one\n[00:02.250] two\n[01:00.00] three");
        assert_eq!(
            lines,
            vec![
                line(1_500, "one"),
                line(2_250, "two"),
                line(60_000, "three")
            ]
        );
    }

    #[test]
    fn one_line_can_carry_several_timestamps() {
        let lines = parse_lrc("[00:30.00][01:30.00]Chorus");
        assert_eq!(lines, vec![line(30_000, "Chorus"), line(90_000, "Chorus")]);
    }

    #[test]
    fn skips_metadata_empty_lines_and_garbage() {
        let text = "[ar:The Weeknd]\n[ti:Blinding Lights]\n[00:05.00]\n[00:06.00]   \nno timestamp\n[00:07.00]kept";
        assert_eq!(parse_lrc(text), vec![line(7_000, "kept")]);
    }

    #[test]
    fn sorts_out_of_order_lines() {
        let lines = parse_lrc("[00:09.00]late\n[00:01.00]early");
        assert_eq!(lines, vec![line(1_000, "early"), line(9_000, "late")]);
    }

    #[test]
    fn rejects_malformed_timestamps() {
        assert_eq!(parse_timestamp("ar:Artist"), None);
        assert_eq!(parse_timestamp("1:2:3"), None);
        assert_eq!(parse_timestamp("00:xx.00"), None);
        assert_eq!(parse_timestamp("00:01."), Some(1_000));
        assert_eq!(parse_timestamp("00:01.5"), Some(1_500));
    }

    #[test]
    fn strips_video_decoration_from_titles() {
        assert_eq!(
            clean_track("Blinding Lights (Official Video)", "The Weeknd"),
            ("Blinding Lights".into(), "The Weeknd".into())
        );
        assert_eq!(
            clean_track("Song [Official Music Video] (Lyrics)", "Artist - Topic"),
            ("Song".into(), "Artist".into())
        );
        assert_eq!(
            clean_track("Song (Audio) (Visualizer)", "X"),
            ("Song".into(), "X".into())
        );
        assert_eq!(
            clean_track("Song (feat. Drake)", "X"),
            ("Song".into(), "X".into())
        );
    }

    #[test]
    fn keeps_meaningful_brackets() {
        assert_eq!(
            clean_track("Keep Going (Remix)", "X"),
            ("Keep Going (Remix)".into(), "X".into())
        );
        assert_eq!(
            clean_track("Left (Left Hand)", "X"),
            ("Left (Left Hand)".into(), "X".into())
        );
    }

    #[test]
    fn splits_artist_and_song_from_title() {
        assert_eq!(
            clean_track("The Weeknd - Blinding Lights (Official Video)", ""),
            ("Blinding Lights".into(), "The Weeknd".into())
        );
    }

    #[test]
    fn leaves_plain_titles_with_no_artist_alone() {
        assert_eq!(clean_track("Song", ""), ("Song".into(), String::new()));
        assert_eq!(clean_track("  ", "  "), (String::new(), String::new()));
    }

    #[test]
    fn track_key_ignores_case_and_spacing() {
        assert_eq!(
            track_key("  Blinding   Lights ", "The Weeknd"),
            track_key("blinding lights", "the weeknd")
        );
    }

    #[test]
    fn query_needs_an_active_track_with_a_title() {
        let mut update = MediaUpdate {
            active: true,
            title: "Song (Lyrics)".into(),
            artist: "Artist".into(),
            duration_ms: 200_400,
            ..MediaUpdate::default()
        };
        let query = Query::from_media(&update).expect("active track has a query");
        assert_eq!(query.title, "Song");
        assert_eq!(query.duration_s, Some(200));

        update.active = false;
        assert_eq!(Query::from_media(&update), None);

        update.active = true;
        update.title = "(Official Video)".into();
        assert_eq!(Query::from_media(&update), None);
    }

    #[test]
    fn unknown_duration_stays_unknown() {
        let update = MediaUpdate {
            active: true,
            title: "Song".into(),
            ..MediaUpdate::default()
        };
        assert_eq!(Query::from_media(&update).map(|q| q.duration_s), Some(None));
    }

    #[test]
    fn best_hit_without_duration_is_the_first_with_lyrics() {
        let results = vec![
            record(None, None, Some(200.0)),
            record(Some("[00:01.00]a"), None, Some(300.0)),
            record(Some("[00:01.00]b"), None, Some(100.0)),
        ];
        let best = pick_best(results, None).expect("a hit with lyrics");
        assert_eq!(best.synced_lyrics.as_deref(), Some("[00:01.00]a"));
    }

    #[test]
    fn best_hit_with_duration_is_closest_within_tolerance() {
        let results = vec![
            record(Some("[00:01.00]far"), None, Some(210.0)),
            record(Some("[00:01.00]near"), None, Some(201.0)),
        ];
        let best = pick_best(results, Some(200)).expect("a close hit");
        assert_eq!(best.synced_lyrics.as_deref(), Some("[00:01.00]near"));
    }

    #[test]
    fn no_hit_when_every_duration_is_out_of_tolerance() {
        let results = vec![record(Some("[00:01.00]far"), None, Some(240.0))];
        assert!(pick_best(results, Some(200)).is_none());
    }

    #[test]
    fn synced_record_becomes_synced_update() {
        let update = to_update("k", Some(record(Some("[00:01.00]hi"), Some("hi"), None)));
        assert_eq!(update.status, Status::Synced);
        assert_eq!(update.lines, vec![line(1_000, "hi")]);
        assert_eq!(update.plain, None);
    }

    #[test]
    fn plain_only_record_becomes_plain_update() {
        let update = to_update("k", Some(record(None, Some("  words  "), None)));
        assert_eq!(update.status, Status::Plain);
        assert_eq!(update.plain.as_deref(), Some("words"));
        assert!(update.lines.is_empty());
    }

    #[test]
    fn missing_or_empty_record_is_none() {
        assert_eq!(to_update("k", None).status, Status::None);
        assert_eq!(
            to_update("k", Some(record(Some(""), Some("   "), None))).status,
            Status::None
        );
    }

    #[test]
    fn cache_returns_hits_and_evicts_the_least_recent() {
        let mut state = LyricsState::default();
        for index in 0..CACHE_SIZE {
            state.remember(LyricsUpdate::none(&format!("t{index}")));
        }
        // Touching t0 makes t1 the oldest entry.
        assert!(state.cached("t0").is_some());
        state.remember(LyricsUpdate::none("new"));

        assert_eq!(state.cache.len(), CACHE_SIZE);
        assert!(state.cached("t0").is_some());
        assert!(state.cached("t1").is_none());
        assert!(state.cached("new").is_some());
    }

    #[test]
    fn cache_replaces_an_entry_with_the_same_key() {
        let mut state = LyricsState::default();
        state.remember(LyricsUpdate::none("a"));
        state.remember(LyricsUpdate {
            key: "a".into(),
            status: Status::Plain,
            lines: Vec::new(),
            plain: Some("x".into()),
        });
        assert_eq!(state.cache.len(), 1);
        assert_eq!(state.cached("a").map(|u| u.status), Some(Status::Plain));
    }

    /// Hits lrclib.net. Run with `cargo test -- --ignored fetches_synced`.
    #[test]
    #[ignore = "needs network access to lrclib.net"]
    fn fetches_synced_lyrics_from_lrclib() {
        let query = Query {
            key: track_key("Blinding Lights", "The Weeknd"),
            title: "Blinding Lights".into(),
            artist: "The Weeknd".into(),
            album: "After Hours".into(),
            duration_s: Some(200),
        };
        let update = to_update(&query.key, fetch(&query).expect("request succeeds"));
        assert_eq!(update.status, Status::Synced);
        assert!(update.lines.len() > 20, "got {} lines", update.lines.len());
    }
}
