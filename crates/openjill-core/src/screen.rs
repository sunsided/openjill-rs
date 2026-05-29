//! Screen handler trait, tick result, and screen transition types.

use crate::ActiveInput;
use crate::render::RenderCommand;
use crate::runtime::RuntimeState;

/// A sound event emitted by a screen handler during one tick.
///
/// Extend as the audio epic defines specific sound identifiers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundEvent {}

/// Result produced by one call to [`ScreenHandler::tick`].
pub struct TickResult {
    /// Render commands to execute in order this tick.
    pub commands: Vec<RenderCommand>,
    /// Optional transition to a different screen, applied after rendering.
    pub transition: Option<ScreenTransition>,
    /// Sound events to play this tick.
    pub sound_events: Vec<SoundEvent>,
}

impl TickResult {
    /// Creates a tick result with no render commands, no transition, and no sound events.
    pub fn empty() -> Self {
        Self {
            commands: Vec::new(),
            transition: None,
            sound_events: Vec::new(),
        }
    }
}

/// Identifies the next screen to display after a screen transition.
///
/// Returned by [`ScreenHandler::tick`] inside [`TickResult::transition`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenTransition {
    /// Return to the main start menu.
    StartMenu,
    /// Show the world map.
    Map,
    /// Load and play a specific level.
    Level {
        /// JN filename for the level, e.g. `"JN1L01.JN1"`.
        file: String,
        /// Level index.
        number: i32,
    },
    /// Restart the current level from the beginning.
    RestartLevel,
    /// Show the story screen (backed by `INTRO.JN1`).
    Story,
    /// Show the credits screen (backed by `INTRO.JN1`).
    Credits,
    /// Show the ordering information screen (backed by `INTRO.JN1`).
    OrderingInfo,
    /// Show the noisemaker screen (backed by `INTRO.JN1`).
    Noisemaker,
    /// Exit the application.
    Quit,
}

/// One loaded screen; advances state and emits render commands on each tick.
///
/// The `ScreenHandler::tick` signature is stable after issue 44 and must not
/// change once the gameplay epic begins.
pub trait ScreenHandler {
    /// Advance the screen state by one fixed tick.
    ///
    /// Returns render commands for this tick and an optional transition to a
    /// new screen. The orchestrator applies the transition after capturing the
    /// commands from this result; the commands themselves come from the
    /// outgoing handler and are presented by the caller before the next tick
    /// uses the new handler.
    fn tick(&mut self, input: &ActiveInput, state: &mut RuntimeState) -> TickResult;

    /// Serializes the handler's current world-map JN bytes to an in-memory
    /// buffer for save and restore operations.
    ///
    /// Returns `None` for screens that do not own the map JN file (the start
    /// menu, intro screens, and level screens). Only `MapScreen` overrides
    /// this method, mirroring the `putCurrentLevelInFileMemory` semantics from
    /// the Java reference for the world-map round-trip.
    fn map_jn_bytes(&self) -> Option<Vec<u8>> {
        None
    }

    /// Serializes the handler's current level JN bytes to an in-memory buffer
    /// for restart-level operations.
    ///
    /// Returns `None` for screens that do not own a level JN file. Only
    /// `LevelScreen` overrides this method, allowing the orchestrator to
    /// reload the same level from memory when a `DieRestartLevel` message
    /// fires.
    fn level_jn_bytes(&self) -> Option<Vec<u8>> {
        None
    }

    /// Serializes a **live** save-game snapshot of this screen's JN file:
    /// the current background layer, the live object entities, and a save-data
    /// block built from `state` (level/health/inventory/score).
    ///
    /// Differs from [`map_jn_bytes`] / [`level_jn_bytes`], which return the
    /// load-time bytes for restart purposes. Only `LevelScreen` (acting as a
    /// regular level or the world map) overrides this; menus and intro screens
    /// return `None`, meaning "nothing to snapshot".
    ///
    /// [`map_jn_bytes`]: ScreenHandler::map_jn_bytes
    /// [`level_jn_bytes`]: ScreenHandler::level_jn_bytes
    fn snapshot_jn_bytes(&self, _state: &RuntimeState) -> Option<Vec<u8>> {
        None
    }

    /// Returns `true` when this screen is the world map (rather than a regular
    /// level).
    ///
    /// Lets the orchestrator decide, without cloning a JN buffer, whether a
    /// live [`snapshot_jn_bytes`] belongs in the map or level half of a save.
    /// Only `LevelScreen` acting as the world map overrides this.
    ///
    /// [`snapshot_jn_bytes`]: ScreenHandler::snapshot_jn_bytes
    fn is_world_map(&self) -> bool {
        false
    }
}
