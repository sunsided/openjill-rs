//! Level configuration bundle passed to screen handlers on construction.

use openjill_core::ScreenTransition;

/// Configuration bundle passed to a screen handler when it is constructed.
///
/// Carries the JN filename, level number, the transition to return to on
/// Escape or Quit, carry-in score and gem count, and optional serialized JN
/// byte arrays for save and restart operations.
pub struct LevelConfig {
    /// JN filename for this screen, e.g. `"INTRO.JN1"`, or `None` for screens
    /// that do not use a JN file.
    pub jn_file: Option<String>,
    /// Level index for this screen; `MAP_LEVEL` for the world-map screen.
    pub level_number: i32,
    /// Transition applied when the player presses Escape or selects Quit from
    /// within this screen.
    pub start_screen: ScreenTransition,
    /// Score carried in from the previous screen or save slot.
    pub carry_score: i32,
    /// Gem count carried in from the previous screen or save slot.
    pub carry_gems: i32,
    /// Serialized world-map JN bytes preserved across screen transitions for
    /// save-slot and restore operations.
    pub map_jn_bytes: Option<Vec<u8>>,
    /// Serialized current-level JN bytes preserved for restart-level operations.
    pub level_jn_bytes: Option<Vec<u8>>,
}
