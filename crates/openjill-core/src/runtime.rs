//! Persistent game state carried across screen transitions.

/// Sentinel level number representing the world-map screen.
///
/// Mirrors `SaveData.MAP_LEVEL = -1` from the Java reference implementation.
pub const MAP_LEVEL: i32 = -1;

/// An item the player can carry in their inventory.
///
/// Extend as the gameplay epic adds more collectible types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryObject {
    /// The Jill character token.
    Jill,
    /// Gem collectible.
    Gem,
    /// Key collectible.
    Key,
    /// Fire flower collectible.
    FireFlower,
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
    /// Number of gems collected.
    pub gem_count: i32,
    /// Items currently held in the player's inventory.
    pub inventory: Vec<InventoryObject>,
}

impl RuntimeState {
    /// Creates default runtime state for the start of episode 1.
    pub fn new() -> Self {
        Self {
            level: MAP_LEVEL,
            score: 0,
            lives: 3,
            gem_count: 0,
            inventory: Vec::new(),
        }
    }
}

impl Default for RuntimeState {
    /// Returns default runtime state for the start of episode 1.
    fn default() -> Self {
        Self::new()
    }
}
