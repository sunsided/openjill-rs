//! Persistent game state carried across screen transitions.

/// Sentinel level number representing the world-map screen.
///
/// Mirrors `SaveData.MAP_LEVEL = -1` from the Java reference implementation.
pub const MAP_LEVEL: i32 = -1;

/// An item the player can carry in their inventory.
///
/// Mirrors the Java reference's `EnumInventoryObject` exactly, including the
/// integer index each variant occupies in the JN save-data inventory block and
/// in `BonusManager` records.  The discriminant **is** that index, so
/// [`InventoryObject::index`] / [`InventoryObject::from_index`] round-trip
/// through the on-disk encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryObject {
    /// The Jill character token (`JILL`, index 0).
    Jill = 0,
    /// Red key collectible (`RED_KEY`, index 1).
    RedKey = 1,
    /// Knife weapon pickup (`KNIVE`, index 2).
    Knife = 2,
    /// Gem collectible (`GEM`, index 3).
    Gem = 3,
    /// Frog collectible / player-form token (`FROG`, index 4).
    Frog = 4,
    /// Firebird transform power-up (`FIREBIRD`, index 5); has no inventory icon.
    Firebird = 5,
    /// Bag-of-coins collectible (`BAG_OF_COIN`, index 6).
    BagOfCoin = 6,
    /// Fish collectible (`FISH`, index 7).
    Fish = 7,
    /// Blade weapon pickup (`BLADE`, index 8).
    Blade = 8,
    /// High-jump power-up (`HIGH_JUMP`, index 9).
    HighJump = 9,
    /// Invincibility power-up (`INVINCIBILITY`, index 10).
    Invincibility = 10,
}

impl InventoryObject {
    /// Returns the JN / `EnumInventoryObject` index for this item (the value
    /// stored in save-data inventory slots and `BonusManager` `counter`).
    pub fn index(self) -> u16 {
        self as u16
    }

    /// Resolves a JN inventory index to its variant, or `None` when the index
    /// is outside the known `EnumInventoryObject` range (0..=10).
    pub fn from_index(index: u16) -> Option<Self> {
        Some(match index {
            0 => Self::Jill,
            1 => Self::RedKey,
            2 => Self::Knife,
            3 => Self::Gem,
            4 => Self::Frog,
            5 => Self::Firebird,
            6 => Self::BagOfCoin,
            7 => Self::Fish,
            8 => Self::Blade,
            9 => Self::HighJump,
            10 => Self::Invincibility,
            _ => return None,
        })
    }
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
    /// Sound ("NOISE") toggle; `true` = sound on.  Shown by the control-panel
    /// noise indicator and flipped by the NOISE key.
    pub noise_enabled: bool,
    /// "TURTLE" slow-motion toggle; `true` = turtle mode on.  Shown by the
    /// control-panel turtle indicator and flipped by the TURTLE key.
    pub turtle_enabled: bool,
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
            noise_enabled: true,
            turtle_enabled: false,
        }
    }
}

impl Default for RuntimeState {
    /// Returns default runtime state for the start of episode 1.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::InventoryObject;

    /// All `EnumInventoryObject` indices in JN order, paired with their variant.
    const INDEXED: [(u16, InventoryObject); 11] = [
        (0, InventoryObject::Jill),
        (1, InventoryObject::RedKey),
        (2, InventoryObject::Knife),
        (3, InventoryObject::Gem),
        (4, InventoryObject::Frog),
        (5, InventoryObject::Firebird),
        (6, InventoryObject::BagOfCoin),
        (7, InventoryObject::Fish),
        (8, InventoryObject::Blade),
        (9, InventoryObject::HighJump),
        (10, InventoryObject::Invincibility),
    ];

    /// Unit under test: [`InventoryObject::index`] / [`InventoryObject::from_index`].
    ///
    /// Invariants asserted: every variant's `index()` equals its Java
    /// `EnumInventoryObject` index, `from_index` is its inverse across the full
    /// range, and an out-of-range index resolves to `None`.
    #[test]
    fn inventory_index_round_trips_against_enum_inventory_object() {
        for (index, item) in INDEXED {
            assert_eq!(item.index(), index, "{item:?} index");
            assert_eq!(
                InventoryObject::from_index(index),
                Some(item),
                "from_index({index})"
            );
        }
        assert_eq!(InventoryObject::from_index(11), None);
        assert_eq!(InventoryObject::from_index(u16::MAX), None);
    }
}
