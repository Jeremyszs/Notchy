//! Island state machine. Hover, media playback, screen presence and transient overlays feed in;
//! the resolved state is emitted to the frontend and read by the cursor watcher for hit testing.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IslandState {
    #[default]
    Idle,
    Active,
    Expanded,
    /// Hovered while the media page has synced lyrics. A wider Expanded with the lyric stack.
    Lyrics,
    /// A foreground window sits under the notch. Shrunk to a sliver so it stays out of the way.
    Tucked,
    /// A transient overlay is up, such as the volume and brightness HUD. Slim, and gone after a
    /// moment. Its content travels on its own event; this state only says it is on screen.
    Hud,
    /// A fullscreen app owns the monitor. Invisible and never hit.
    Hidden,
}

impl IslandState {
    /// Logical pill size. Mirrors SIZES in src/Island.svelte; keep them in sync.
    pub fn pill_size(self) -> (f64, f64) {
        match self {
            Self::Idle => (120.0, 30.0),
            Self::Active => (200.0, 30.0),
            Self::Expanded => (380.0, 184.0),
            Self::Lyrics => (620.0, 214.0),
            Self::Hud => (260.0, 30.0),
            Self::Tucked => (64.0, 5.0),
            Self::Hidden => (0.0, 0.0),
        }
    }
}

/// Precedence, highest first: fullscreen hides everything, hover expands (to the lyrics layout
/// when it is wide), a transient overlay shows over the rest, a covered notch tucks, media shows
/// the active pill, and otherwise the island idles. A HUD never takes over a hover or fullscreen.
pub fn resolve(
    hovered: bool,
    media_playing: bool,
    fullscreen: bool,
    covered: bool,
    wide: bool,
    transient: bool,
) -> IslandState {
    if fullscreen {
        IslandState::Hidden
    } else if hovered && wide {
        IslandState::Lyrics
    } else if hovered {
        IslandState::Expanded
    } else if transient {
        IslandState::Hud
    } else if covered {
        IslandState::Tucked
    } else if media_playing {
        IslandState::Active
    } else {
        IslandState::Idle
    }
}

#[derive(Debug, Default)]
pub struct IslandStateMachine {
    hovered: bool,
    media_playing: bool,
    fullscreen: bool,
    covered: bool,
    wide: bool,
    transient: bool,
}

impl IslandStateMachine {
    pub fn current(&self) -> IslandState {
        resolve(
            self.hovered,
            self.media_playing,
            self.fullscreen,
            self.covered,
            self.wide,
            self.transient,
        )
    }

    pub fn set_hovered(&mut self, hovered: bool) {
        self.hovered = hovered;
    }

    pub fn set_media_playing(&mut self, playing: bool) {
        self.media_playing = playing;
    }

    pub fn set_fullscreen(&mut self, fullscreen: bool) {
        self.fullscreen = fullscreen;
    }

    pub fn set_covered(&mut self, covered: bool) {
        self.covered = covered;
    }

    pub fn set_wide(&mut self, wide: bool) {
        self.wide = wide;
    }

    /// Whether a transient overlay (the HUD) is on screen. Owned by `modules::hud`.
    pub fn set_transient(&mut self, transient: bool) {
        self.transient = transient;
    }
}

pub type SharedIsland = Arc<Mutex<IslandStateMachine>>;

pub fn lock(island: &SharedIsland) -> MutexGuard<'_, IslandStateMachine> {
    // The machine has no invariant a panic could break, so a poisoned lock is still safe to use.
    island.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Applies a change under the lock and emits the new state only if it differs.
/// Emitting after the guard drops keeps the cursor watcher off the event path.
pub fn commit(
    app: &AppHandle,
    island: &SharedIsland,
    change: impl FnOnce(&mut IslandStateMachine),
) {
    let changed = {
        let mut machine = lock(island);
        let before = machine.current();
        change(&mut machine);
        let after = machine.current();
        (after != before).then_some(after)
    };

    if let Some(state) = changed {
        let _ = app.emit("island://state", state);
    }
}

/// Tells the machine whether the media page has synced lyrics to show. Hover still decides
/// whether the wide layout appears, so leaving the island needs no help from the frontend.
#[tauri::command]
pub fn island_set_wide(app: AppHandle, island: State<'_, SharedIsland>, wide: bool) {
    commit(&app, &island, |machine| machine.set_wide(wide));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination of inputs, in precedence order, with the state it must resolve to.
    /// Columns: hovered, media_playing, fullscreen, covered, wide, transient -> expected.
    #[test]
    fn resolve_follows_precedence_for_every_input_combination() {
        use IslandState::*;
        #[rustfmt::skip]
        let table = [
            (false, false, false, false, false, false, Idle),
            (false, false, false, true , false, false, Tucked),
            (false, false, true , false, false, false, Hidden),
            (false, false, true , true , false, false, Hidden),
            (false, true , false, false, false, false, Active),
            (false, true , false, true , false, false, Tucked),
            (false, true , true , false, false, false, Hidden),
            (false, true , true , true , false, false, Hidden),
            (true , false, false, false, false, false, Expanded),
            (true , false, false, true , false, false, Expanded),
            (true , false, true , false, false, false, Hidden),
            (true , false, true , true , false, false, Hidden),
            (true , true , false, false, false, false, Expanded),
            (true , true , false, true , false, false, Expanded),
            (true , true , true , false, false, false, Hidden),
            (true , true , true , true , false, false, Hidden),
            (false, false, false, false, false, true , Hud),
            (false, false, false, true , false, true , Hud),
            (false, false, true , false, false, true , Hidden),
            (false, false, true , true , false, true , Hidden),
            (false, true , false, false, false, true , Hud),
            (false, true , false, true , false, true , Hud),
            (false, true , true , false, false, true , Hidden),
            (false, true , true , true , false, true , Hidden),
            (true , false, false, false, false, true , Expanded),
            (true , false, false, true , false, true , Expanded),
            (true , false, true , false, false, true , Hidden),
            (true , false, true , true , false, true , Hidden),
            (true , true , false, false, false, true , Expanded),
            (true , true , false, true , false, true , Expanded),
            (true , true , true , false, false, true , Hidden),
            (true , true , true , true , false, true , Hidden),
            (false, false, false, false, true , false, Idle),
            (false, false, false, true , true , false, Tucked),
            (false, false, true , false, true , false, Hidden),
            (false, false, true , true , true , false, Hidden),
            (false, true , false, false, true , false, Active),
            (false, true , false, true , true , false, Tucked),
            (false, true , true , false, true , false, Hidden),
            (false, true , true , true , true , false, Hidden),
            (true , false, false, false, true , false, Lyrics),
            (true , false, false, true , true , false, Lyrics),
            (true , false, true , false, true , false, Hidden),
            (true , false, true , true , true , false, Hidden),
            (true , true , false, false, true , false, Lyrics),
            (true , true , false, true , true , false, Lyrics),
            (true , true , true , false, true , false, Hidden),
            (true , true , true , true , true , false, Hidden),
            (false, false, false, false, true , true , Hud),
            (false, false, false, true , true , true , Hud),
            (false, false, true , false, true , true , Hidden),
            (false, false, true , true , true , true , Hidden),
            (false, true , false, false, true , true , Hud),
            (false, true , false, true , true , true , Hud),
            (false, true , true , false, true , true , Hidden),
            (false, true , true , true , true , true , Hidden),
            (true , false, false, false, true , true , Lyrics),
            (true , false, false, true , true , true , Lyrics),
            (true , false, true , false, true , true , Hidden),
            (true , false, true , true , true , true , Hidden),
            (true , true , false, false, true , true , Lyrics),
            (true , true , false, true , true , true , Lyrics),
            (true , true , true , false, true , true , Hidden),
            (true , true , true , true , true , true , Hidden),
        ];

        for (hovered, media, fullscreen, covered, wide, transient, expected) in table {
            assert_eq!(
                resolve(hovered, media, fullscreen, covered, wide, transient),
                expected,
                "hovered={hovered} media={media} fullscreen={fullscreen} covered={covered} wide={wide} transient={transient}"
            );
        }
    }

    #[test]
    fn machine_tracks_inputs() {
        let mut machine = IslandStateMachine::default();
        assert_eq!(machine.current(), IslandState::Idle);

        machine.set_media_playing(true);
        assert_eq!(machine.current(), IslandState::Active);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_media_playing(false);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Idle);
    }

    #[test]
    fn wide_shows_lyrics_only_while_hovered() {
        let mut machine = IslandStateMachine::default();
        machine.set_media_playing(true);
        machine.set_wide(true);
        assert_eq!(machine.current(), IslandState::Active);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Lyrics);

        machine.set_wide(false);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_wide(true);
        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Active);
    }

    #[test]
    fn covered_tucks_until_something_takes_priority() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        assert_eq!(machine.current(), IslandState::Tucked);

        machine.set_media_playing(true);
        assert_eq!(machine.current(), IslandState::Tucked);

        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);

        machine.set_fullscreen(false);
        machine.set_hovered(false);
        machine.set_covered(false);
        assert_eq!(machine.current(), IslandState::Active);
    }

    /// A HUD shows over a tucked or playing island, then gives way when it ends.
    #[test]
    fn transient_overlay_shows_over_tucked_and_active_then_returns() {
        let mut machine = IslandStateMachine::default();
        machine.set_covered(true);
        machine.set_media_playing(true);
        machine.set_transient(true);
        assert_eq!(machine.current(), IslandState::Hud);

        machine.set_transient(false);
        assert_eq!(machine.current(), IslandState::Tucked);
    }

    /// Hover and fullscreen both outrank the HUD.
    #[test]
    fn hover_and_fullscreen_outrank_the_transient_overlay() {
        let mut machine = IslandStateMachine::default();
        machine.set_transient(true);
        machine.set_hovered(true);
        assert_eq!(machine.current(), IslandState::Expanded);

        machine.set_wide(true);
        assert_eq!(machine.current(), IslandState::Lyrics);

        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);

        machine.set_hovered(false);
        assert_eq!(machine.current(), IslandState::Hidden);
    }

    #[test]
    fn fullscreen_hides_regardless_of_other_inputs() {
        let mut machine = IslandStateMachine::default();
        machine.set_hovered(true);
        machine.set_media_playing(true);
        machine.set_covered(true);
        machine.set_wide(true);
        machine.set_transient(true);
        machine.set_fullscreen(true);
        assert_eq!(machine.current(), IslandState::Hidden);
    }

    /// The window is 240 px tall (tauri.conf.json). The expanded pill, with its pager row, must
    /// leave room for it.
    #[test]
    fn expanded_pill_fits_the_window() {
        let (_, h) = IslandState::Expanded.pill_size();
        assert!(h <= 240.0, "expanded pill is {h} px tall");
    }

    /// The window is 720 px wide and 240 px tall (tauri.conf.json), so the lyrics pill must stay
    /// inside it, with room left for the outline's bleed above the top edge.
    #[test]
    fn lyrics_pill_fits_the_window() {
        let (w, h) = IslandState::Lyrics.pill_size();
        assert_eq!((w, h), (620.0, 214.0));
        assert!(w <= 720.0, "lyrics pill is {w} px wide");
        assert!(h <= 240.0, "lyrics pill is {h} px tall");
    }

    #[test]
    fn hud_pill_is_slim_and_fits_the_window() {
        let (w, h) = IslandState::Hud.pill_size();
        assert_eq!((w, h), (260.0, 30.0));
        assert!(w <= 720.0 && h <= 240.0);
    }

    #[test]
    fn hidden_has_no_size() {
        assert_eq!(IslandState::Hidden.pill_size(), (0.0, 0.0));
        assert_eq!(IslandState::Tucked.pill_size(), (64.0, 5.0));
    }
}
