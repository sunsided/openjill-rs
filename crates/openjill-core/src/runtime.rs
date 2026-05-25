//! Persistent game state carried across screen transitions.

/// Sentinel level number representing the world-map screen.
///
/// Mirrors `SaveData.MAP_LEVEL = -1` from the Java reference implementation.
pub const MAP_LEVEL: i32 = -1;

/// An item the player can carry in their inventory.
///
/// Mirrors the inventory icon set from the Java reference's
/// `InventoryItemMessage` enum.  Episode 1 uses all variants listed below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryObject {
    /// The Jill character token.
    Jill,
    /// Gem collectible.
    Gem,
    /// Key collectible (red or rock variants share this icon).
    Key,
    /// Knife weapon pickup.
    Knife,
    /// Blade weapon pickup.
    Blade,
    /// Fire flower collectible.
    FireFlower,
    /// Firebird transform power-up.
    Firebird,
}

/// Persistent state carried across screen transitions.
///
/// Constructed once at application start and mutated in place as the player
/// moves between screens and levels.
#[derive(Clone, Debug)]
pub struct RuntimeState {
    /// Current level number; `MAP_LEVEL` for the world-map screen.
    pub level: i32,
    /// Accumulated score.
    pub score: i32,
    /// Remaining lives.
    pub lives: i32,
    /// Current health (lifebar segment count).
    ///
    /// Starts at `defaultLife = 6` from `inventory_conf.json`; maximum is 8.
    pub health: i32,
    /// Number of gems collected.
    pub gem_count: i32,
    /// Items currently held in the player's inventory.
    ///
    /// Slot 0 is always the active player-form token (`Jill` at start).
    pub inventory: Vec<InventoryObject>,
    /// Remaining player-damage invincibility ticks.
    ///
    /// Decremented once per tick by the level loop. While non-zero,
    /// enemy contact armed via `take_player_kill` is ignored so a single
    /// touch deals exactly one point of damage even when the player and
    /// enemy bounding boxes overlap for multiple consecutive ticks. The
    /// counter is reset to [`PLAYER_INVINCIBILITY_TICKS`] whenever a hit
    /// is actually applied.
    pub invincibility_ticks: i32,
}

/// Player-side damage cooldown applied after a successful enemy hit.
///
/// REVERSE-ENGINEERED: tuned for episode-1 playthrough so the player can
/// step away from an enemy after taking one point of damage instead of
/// losing the full life bar within a second. The Java reference relies
/// on the per-enemy `zapholdValueAfterTouchPlayer = 3` cooldown only,
/// which is too short for the Rust port's collision rate. Future engine
/// config file should expose this value.
pub const PLAYER_INVINCIBILITY_TICKS: i32 = 30;

impl RuntimeState {
    /// Creates default runtime state for the start of episode 1.
    pub fn new() -> Self {
        Self {
            level: MAP_LEVEL,
            score: 0,
            lives: 3,
            health: 6,
            gem_count: 0,
            inventory: vec![InventoryObject::Jill],
            invincibility_ticks: 0,
        }
    }
}

impl Default for RuntimeState {
    /// Returns default runtime state for the start of episode 1.
    fn default() -> Self {
        Self::new()
    }
}
