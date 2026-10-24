//! Media session tracking via Windows GlobalSystemMediaTransportControls.
//!
//! One worker thread owns every WinRT object. Event handlers run on thread-pool threads
//! and only queue a message, and playback commands travel the same queue, so nothing
//! WinRT has to cross threads.

use std::future::IntoFuture;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize};
use tauri::async_runtime::block_on;
use tauri::{AppHandle, Emitter, Manager};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as SessionManager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as MediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs, SessionsChangedEventArgs,
    TimelinePropertiesChangedEventArgs,
};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};
use windows::core::{Result as WinResult, RuntimeType};

use super::{Module, ModuleContext};
use crate::state;

const UPDATE_EVENT: &str = "media://update";
// Art is shipped over IPC on every change, so oversized covers are skipped rather than sent.
const MAX_ART_BYTES: u64 = 1_500_000;

/// The frontend contract. Mirrors MediaState in src/stores/media.ts.
///
/// `position_ms` was sampled at `position_at_ms` (unix epoch ms), so the frontend can
/// extrapolate while playing. `duration_ms` is 0 when the app reports no usable length.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaUpdate {
    pub active: bool,
    pub playing: bool,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub source: String,
    pub art: Option<String>,
    pub duration_ms: u64,
    pub position_ms: u64,
    pub position_at_ms: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Control {
    PlayPause,
    Next,
    Previous,
    Seek(u64),
}

#[derive(Debug, Clone, Copy)]
enum Message {
    SessionsChanged,
    Refresh,
    Control(Control),
}

/// Shared with the Tauri commands: the queue into the worker, and the last update sent,
/// so a frontend that starts late can read it.
struct MediaHub {
    commands: Sender<Message>,
    latest: Mutex<MediaUpdate>,
}

pub struct MediaModule;

impl Module for MediaModule {
    fn name(&self) -> &'static str {
        "media"
    }

    fn start(&self, ctx: ModuleContext) {
        let (commands, inbox) = mpsc::channel();
        let hub = Arc::new(MediaHub {
            commands,
            latest: Mutex::default(),
        });
        ctx.app.manage(hub.clone());
        thread::spawn(move || worker(ctx, hub, inbox));
    }
}

fn worker(ctx: ModuleContext, hub: Arc<MediaHub>, inbox: Receiver<Message>) {
    // Dedicated thread with no message pump, so MTA is the right apartment. Initialization
    // failure means WinRT is unavailable and there is nothing to track.
    let Some(_com) = ComGuard::init() else {
        return;
    };
    let Ok(manager) = wait(SessionManager::RequestAsync()) else {
        return;
    };
    // Sessions come and go, and the current one is the fallback when none is playing.
    let _ = manager.SessionsChanged(&queue::<SessionManager, SessionsChangedEventArgs>(
        &hub.commands,
        Message::SessionsChanged,
    ));
    let _ = manager.CurrentSessionChanged(
        &queue::<SessionManager, CurrentSessionChangedEventArgs>(&hub.commands, Message::Refresh),
    );
    // Queue the first sync so the initial sessions are read through the same path.
    let _ = hub.commands.send(Message::SessionsChanged);

    let mut tracked: Vec<Tracked> = Vec::new();
    let mut art = ArtCache::default();

    for message in inbox {
        match message {
            Message::SessionsChanged => {
                for old in tracked.drain(..) {
                    old.detach();
                }
                tracked = attach_all(&manager, &hub.commands);
                refresh(&ctx, &hub, &manager, &tracked, &mut art);
            }
            Message::Refresh => {
                refresh(&ctx, &hub, &manager, &tracked, &mut art);
            }
            Message::Control(control) => {
                if let Some(session) = chosen(&manager, &tracked) {
                    run_control(session, control);
                }
            }
        }
    }
}

/// A session we are subscribed to, and the tokens that remove those subscriptions. A token
/// is missing when its subscription failed, so the session just reports fewer changes.
struct Tracked {
    session: Session,
    properties: Option<i64>,
    playback: Option<i64>,
    timeline: Option<i64>,
}

impl Tracked {
    fn attach(session: Session, commands: &Sender<Message>) -> Self {
        let properties = session
            .MediaPropertiesChanged(&queue::<Session, MediaPropertiesChangedEventArgs>(
                commands,
                Message::Refresh,
            ))
            .ok();
        let playback = session
            .PlaybackInfoChanged(&queue::<Session, PlaybackInfoChangedEventArgs>(
                commands,
                Message::Refresh,
            ))
            .ok();
        let timeline = session
            .TimelinePropertiesChanged(&queue::<Session, TimelinePropertiesChangedEventArgs>(
                commands,
                Message::Refresh,
            ))
            .ok();
        Self {
            session,
            properties,
            playback,
            timeline,
        }
    }

    fn detach(self) {
        if let Some(token) = self.properties {
            let _ = self.session.RemoveMediaPropertiesChanged(token);
        }
        if let Some(token) = self.playback {
            let _ = self.session.RemovePlaybackInfoChanged(token);
        }
        if let Some(token) = self.timeline {
            let _ = self.session.RemoveTimelinePropertiesChanged(token);
        }
    }

    fn playback_status(&self) -> Option<PlaybackStatus> {
        self.session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()
    }
}

fn attach_all(manager: &SessionManager, commands: &Sender<Message>) -> Vec<Tracked> {
    let Ok(sessions) = manager.GetSessions() else {
        return Vec::new();
    };
    let Ok(count) = sessions.Size() else {
        return Vec::new();
    };
    (0..count)
        .filter_map(|index| sessions.GetAt(index).ok())
        .map(|session| Tracked::attach(session, commands))
        .collect()
}

/// A handler that queues one message per WinRT event. It runs on a thread-pool thread,
/// so it only sends.
fn queue<S, A>(commands: &Sender<Message>, message: Message) -> TypedEventHandler<S, A>
where
    S: RuntimeType + 'static,
    A: RuntimeType + 'static,
{
    let commands = commands.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = commands.send(message);
        Ok(())
    })
}

/// Blocks the worker until a WinRT async operation completes. Safe only because the
/// worker is its own MTA thread, so completions never wait on it.
fn wait<O, T>(op: WinResult<O>) -> WinResult<T>
where
    O: IntoFuture<Output = WinResult<T>>,
{
    block_on(op?.into_future())
}

fn run_control(session: &Session, control: Control) {
    let _ = match control {
        Control::PlayPause => wait(session.TryTogglePlayPauseAsync()),
        Control::Next => wait(session.TrySkipNextAsync()),
        Control::Previous => wait(session.TrySkipPreviousAsync()),
        Control::Seek(ms) => {
            let duration = session
                .GetTimelineProperties()
                .ok()
                .and_then(|timeline| timeline.EndTime().ok())
                .map_or(0, |end| ticks_to_ms(end.Duration));
            if duration == 0 {
                return;
            }
            wait(session.TryChangePlaybackPositionAsync(seek_ticks(ms, duration)))
        }
    };
}

fn seek_ticks(ms: u64, duration_ms: u64) -> i64 {
    let ms = if duration_ms > 0 {
        ms.min(duration_ms)
    } else {
        ms
    };
    ms.saturating_mul(10_000).min(i64::MAX as u64) as i64
}

/// The session shown and controlled: the first one playing, else the current one, else none.
/// Commands and updates both go through here, so they always target the same session.
fn chosen<'a>(manager: &SessionManager, tracked: &'a [Tracked]) -> Option<&'a Session> {
    let current = manager.GetCurrentSession().ok();
    let statuses: Vec<Option<PlaybackStatus>> =
        tracked.iter().map(Tracked::playback_status).collect();
    let current_index =
        current.and_then(|current| tracked.iter().position(|t| t.session == current));
    choose_session(&statuses, current_index).map(|index| &tracked[index].session)
}

/// Pure form of `chosen`. `statuses` has one entry per session, `None` when its status could
/// not be read. `current` is the index of the current session, if it is one of them.
fn choose_session(statuses: &[Option<PlaybackStatus>], current: Option<usize>) -> Option<usize> {
    statuses
        .iter()
        .position(|status| *status == Some(PlaybackStatus::Playing))
        .or_else(|| current.filter(|&index| index < statuses.len()))
}

fn refresh(
    ctx: &ModuleContext,
    hub: &MediaHub,
    manager: &SessionManager,
    tracked: &[Tracked],
    art: &mut ArtCache,
) {
    let update = chosen(manager, tracked)
        .and_then(|session| read_update(session, art))
        .unwrap_or_default();

    state::commit(&ctx.app, &ctx.island, |machine| {
        machine.set_media_playing(update.playing);
    });
    publish(&ctx.app, hub, update);
}

/// Emits only when the payload changed. Emitting under the lock keeps the event order
/// the same as the order of `latest`.
fn publish(app: &AppHandle, hub: &MediaHub, update: MediaUpdate) {
    let mut latest = hub.latest.lock().unwrap_or_else(PoisonError::into_inner);
    if replace_if_changed(&mut latest, update) {
        let _ = app.emit(UPDATE_EVENT, latest.clone());
    }
}

/// Stores `next` as the latest update. Returns false when nothing changed.
fn replace_if_changed(latest: &mut MediaUpdate, next: MediaUpdate) -> bool {
    if *latest == next {
        return false;
    }
    *latest = next;
    true
}

fn read_update(session: &Session, cache: &mut ArtCache) -> Option<MediaUpdate> {
    let properties = wait(session.TryGetMediaPropertiesAsync()).ok()?;
    let title = properties.Title().ok()?.to_string();
    let artist = properties.Artist().ok()?.to_string();
    let album = properties
        .AlbumTitle()
        .map(|album| album.to_string())
        .unwrap_or_default();
    let status = session.GetPlaybackInfo().ok()?.PlaybackStatus().ok()?;
    let art = cache.get_or_load(&title, &artist, || read_art(&properties));
    let source = session
        .SourceAppUserModelId()
        .map(|id| friendly_source(&id.to_string()))
        .unwrap_or_default();
    let (position_ms, duration_ms) = read_timeline(session);

    Some(MediaUpdate {
        active: true,
        playing: status == PlaybackStatus::Playing,
        title,
        artist,
        album,
        source,
        art,
        duration_ms,
        position_ms,
        position_at_ms: unix_ms_now(),
    })
}

/// Position and length in ms. Missing timeline data reads as zero.
fn read_timeline(session: &Session) -> (u64, u64) {
    let Ok(timeline) = session.GetTimelineProperties() else {
        return (0, 0);
    };
    let ticks = |span: WinResult<windows::Foundation::TimeSpan>| {
        span.map(|span| span.Duration).unwrap_or(0)
    };
    timeline_ms(
        ticks(timeline.Position()),
        ticks(timeline.StartTime()),
        ticks(timeline.EndTime()),
    )
}

/// Turns WinRT timeline ticks (100 ns each) into `(position_ms, duration_ms)`. A duration
/// that is zero or negative is reported as 0, which means unknown.
fn timeline_ms(position_ticks: i64, start_ticks: i64, end_ticks: i64) -> (u64, u64) {
    let duration_ticks = end_ticks.saturating_sub(start_ticks).max(0);
    (ticks_to_ms(position_ticks), ticks_to_ms(duration_ticks))
}

fn ticks_to_ms(ticks: i64) -> u64 {
    u64::try_from(ticks / 10_000).unwrap_or(0)
}

fn unix_ms_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0))
}

/// A short name for the app behind a session. Known apps get a fixed name. Otherwise the
/// package part and any `.exe` suffix are dropped, and the raw id is kept when nothing is left.
fn friendly_source(app_id: &str) -> String {
    const KNOWN: [(&str, &str); 7] = [
        ("msedge", "Microsoft Edge"),
        ("microsoftedge", "Microsoft Edge"),
        ("chrome", "Chrome"),
        ("firefox", "Firefox"),
        ("spotify", "Spotify"),
        ("applemusic", "Apple Music"),
        ("zunemusic", "Media Player"),
    ];
    let lower = app_id.to_ascii_lowercase();
    if let Some((_, name)) = KNOWN.iter().find(|(needle, _)| lower.contains(needle)) {
        return (*name).to_owned();
    }

    // Packaged ids look like `Publisher.App_publisherid!AppId`, and desktop ids like `App.exe`.
    let base = app_id.split(['_', '!']).next().unwrap_or(app_id).trim();
    let stem = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base)
        .trim();
    if stem.is_empty() {
        app_id.to_owned()
    } else {
        stem.to_owned()
    }
}

fn read_art(properties: &MediaProperties) -> Option<String> {
    let thumbnail = properties.Thumbnail().ok()?;
    let stream = wait(thumbnail.OpenReadAsync()).ok()?;
    let size = stream.Size().ok()?;
    if size > MAX_ART_BYTES {
        return None;
    }

    let input = stream.GetInputStreamAt(0).ok()?;
    let reader = DataReader::CreateDataReader(&input).ok()?;
    wait(reader.LoadAsync(size as u32)).ok()?;
    let mut bytes = vec![0; size as usize];
    reader.ReadBytes(&mut bytes).ok()?;
    art_data_uri(&bytes)
}

/// Holds the art for the current track, so playback-status changes don't re-read the
/// thumbnail. The key is title plus artist, which is what the session reports.
#[derive(Default)]
struct ArtCache {
    key: Option<(String, String)>,
    art: Option<String>,
}

impl ArtCache {
    fn get_or_load(
        &mut self,
        title: &str,
        artist: &str,
        load: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        let same_track = self
            .key
            .as_ref()
            .is_some_and(|(t, a)| t == title && a == artist);
        if !same_track {
            self.art = load();
            self.key = Some((title.to_owned(), artist.to_owned()));
        }
        self.art.clone()
    }
}

/// Wraps JPEG or PNG bytes as a data URI. Sniffing the magic bytes is more reliable than
/// the stream's content type, which some apps leave empty.
pub(crate) fn art_data_uri(bytes: &[u8]) -> Option<String> {
    let mime = image_mime(bytes)?;
    Some(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else {
        None
    }
}

/// RoInitialize must be paired with RoUninitialize on the same thread. Only the success
/// path (including S_FALSE) increments the count, so only that path gets undone.
struct ComGuard;

impl ComGuard {
    fn init() -> Option<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .ok()
            .map(|()| Self)
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

pub(crate) fn send_control(app: &AppHandle, control: Control) {
    if let Some(hub) = app.try_state::<Arc<MediaHub>>() {
        let _ = hub.commands.send(Message::Control(control));
    }
}

// Commands only enqueue. The worker does the WinRT work and reports the outcome through
// the usual update event, so a failed or unsupported control is silently a no-op.

#[tauri::command]
pub async fn media_play_pause(app: AppHandle) {
    send_control(&app, Control::PlayPause);
}

#[tauri::command]
pub async fn media_next(app: AppHandle) {
    send_control(&app, Control::Next);
}

#[tauri::command]
pub async fn media_previous(app: AppHandle) {
    send_control(&app, Control::Previous);
}

#[tauri::command]
pub async fn media_seek(app: AppHandle, position_ms: u64) {
    send_control(&app, Control::Seek(position_ms));
}

#[tauri::command]
pub fn media_snapshot(app: AppHandle) -> MediaUpdate {
    app.try_state::<Arc<MediaHub>>()
        .map(|hub| {
            hub.latest
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    #[test]
    fn seek_conversion_clamps_and_saturates() {
        assert_eq!(seek_ticks(123, 0), 1_230_000);
        assert_eq!(seek_ticks(200, 100), 1_000_000);
        assert_eq!(seek_ticks(0, 100), 0);
        assert_eq!(seek_ticks(u64::MAX, 0), i64::MAX);
    }

    #[test]
    fn sniffs_jpeg() {
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
    }

    #[test]
    fn sniffs_png() {
        assert_eq!(image_mime(&PNG_HEADER), Some("image/png"));
    }

    #[test]
    fn rejects_other_formats() {
        assert_eq!(image_mime(b"GIF89a"), None);
        assert_eq!(image_mime(&[]), None);
    }

    #[test]
    fn data_uri_carries_mime_and_base64() {
        let uri = art_data_uri(&PNG_HEADER).expect("png is accepted");
        assert_eq!(uri, "data:image/png;base64,iVBORw0KGgo=");
    }

    #[test]
    fn unchanged_update_is_not_replaced() {
        let mut latest = MediaUpdate::default();
        let playing = MediaUpdate {
            active: true,
            playing: true,
            title: "Song".into(),
            artist: "Artist".into(),
            ..MediaUpdate::default()
        };

        assert!(replace_if_changed(&mut latest, playing.clone()));
        assert!(!replace_if_changed(&mut latest, playing.clone()));

        let paused = MediaUpdate {
            playing: false,
            ..playing
        };
        assert!(replace_if_changed(&mut latest, paused));
    }

    #[test]
    fn art_is_read_once_per_track() {
        let mut cache = ArtCache::default();
        let mut loads = 0;

        let first = cache.get_or_load("Song", "Artist", || {
            loads += 1;
            Some("art".into())
        });
        let again = cache.get_or_load("Song", "Artist", || {
            loads += 1;
            None
        });

        assert_eq!(loads, 1);
        assert_eq!(first.as_deref(), Some("art"));
        assert_eq!(again.as_deref(), Some("art"));
    }

    #[test]
    fn new_track_reloads_art() {
        let mut cache = ArtCache::default();
        cache.get_or_load("Song", "Artist", || Some("old".into()));

        let next = cache.get_or_load("Other", "Artist", || Some("new".into()));
        assert_eq!(next.as_deref(), Some("new"));
    }

    #[test]
    fn friendly_source_names_known_apps() {
        assert_eq!(friendly_source("Spotify.exe"), "Spotify");
        assert_eq!(friendly_source("Chrome"), "Chrome");
        assert_eq!(friendly_source("MSEdge"), "Microsoft Edge");
        assert_eq!(
            friendly_source("Microsoft.MicrosoftEdge.Stable_8wekyb3d8bbwe!MSEdge"),
            "Microsoft Edge"
        );
        assert_eq!(
            friendly_source("AppleInc.AppleMusic_nzyj5cx40ttqa!App"),
            "Apple Music"
        );
    }

    #[test]
    fn friendly_source_strips_package_and_exe_for_unknown_apps() {
        assert_eq!(friendly_source("Discord.exe"), "Discord");
        assert_eq!(friendly_source("Foo_8wekyb3d8bbwe!Foo"), "Foo");
        assert_eq!(friendly_source("Tidal"), "Tidal");
    }

    #[test]
    fn friendly_source_keeps_raw_id_when_nothing_is_left() {
        assert_eq!(friendly_source(".exe"), ".exe");
        assert_eq!(friendly_source(""), "");
    }

    #[test]
    fn timeline_converts_ticks_to_milliseconds() {
        // 100 ns ticks: 15,000,000 is 1.5 s, and a 200 s range starts at 0.
        assert_eq!(timeline_ms(15_000_000, 0, 2_000_000_000), (1_500, 200_000));
    }

    #[test]
    fn timeline_treats_empty_or_inverted_range_as_unknown() {
        assert_eq!(timeline_ms(5_000_000, 0, 0), (500, 0));
        assert_eq!(timeline_ms(-10_000, 100, 50), (0, 0));
    }

    #[test]
    fn best_session_is_the_first_playing_one() {
        let statuses = [
            Some(PlaybackStatus::Paused),
            Some(PlaybackStatus::Playing),
            Some(PlaybackStatus::Playing),
        ];
        assert_eq!(choose_session(&statuses, Some(0)), Some(1));
    }

    #[test]
    fn best_session_falls_back_to_current_when_nothing_plays() {
        let statuses = [Some(PlaybackStatus::Paused), Some(PlaybackStatus::Closed)];
        assert_eq!(choose_session(&statuses, Some(1)), Some(1));
        assert_eq!(choose_session(&statuses, None), None);
    }

    #[test]
    fn best_session_is_none_without_sessions_or_a_usable_fallback() {
        assert_eq!(choose_session(&[], None), None);
        assert_eq!(choose_session(&[], Some(0)), None);
        let statuses = [None, Some(PlaybackStatus::Paused)];
        assert_eq!(choose_session(&statuses, None), None);
    }

    #[test]
    fn unreadable_status_does_not_count_as_playing() {
        let statuses = [None, Some(PlaybackStatus::Playing)];
        assert_eq!(choose_session(&statuses, Some(0)), Some(1));
    }
}
