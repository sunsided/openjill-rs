//! Background entity implementations registered against DMA cell names.
//!
//! Each named entry from `background_manager_mapping.properties` that has
//! gameplay behaviour beyond a plain blit lives in its own submodule below.
//! Names not handled explicitly fall through to
//! [`standard::StdBackgroundEntity`], the catch-all cell handler.

pub mod base_tree;
pub mod ffloor;
pub mod froof;
pub mod kill_lava;
pub mod kill_two;
pub mod kill_water;
pub mod spike;
pub mod standard;

use openjill_core::BackgroundEntity;

use crate::asset_cache::AssetCache;

pub use base_tree::BaseTreeBackground;
pub use ffloor::FFloorBackground;
pub use froof::FroofBackground;
pub use kill_lava::KillLavaBackground;
pub use kill_two::Kill2Background;
pub use kill_water::KillWaterBackground;
pub use spike::SpikeBackground;
pub use standard::StdBackgroundEntity;

/// Builds the correct [`BackgroundEntity`] implementation for a DMA cell.
///
/// `dma_name` is the trimmed DMA name string (uppercase ASCII, e.g.
/// `"BASETREE"`).  `map_code` is the masked background map code that selected
/// the DMA entry, used so a specialised entity that needs to round-trip back
/// through `DmaFile::get_by_map_code` can do so.  `cache` supplies shared
/// episode assets (DMA, SHA) required to construct individual entities.
///
/// Names that have no specialised entity fall through to
/// [`StdBackgroundEntity::for_map_code`].
pub fn make_background_entity(
    dma_name: &str,
    map_code: u16,
    cache: &AssetCache,
) -> Box<dyn BackgroundEntity> {
    match dma_name {
        "SPIKE" => Box::new(SpikeBackground::for_map_code(map_code, cache)),
        "BASETREE" => Box::new(BaseTreeBackground::for_map_code(map_code, cache)),
        "FFLOOR" => Box::new(FFloorBackground::for_map_code(map_code, cache)),
        "FROOF" => Box::new(FroofBackground::for_map_code(map_code, cache)),
        "STALACBR" | "STALAGBR" | "WTHORN" | "WTHORN2" => {
            Box::new(Kill2Background::for_map_code(map_code, cache))
        }
        name if is_lava_name(name) => Box::new(KillLavaBackground::for_map_code(map_code, cache)),
        name if is_water_name(name) => Box::new(KillWaterBackground::for_map_code(map_code, cache)),
        _ => Box::new(StdBackgroundEntity::for_map_code(map_code, cache)),
    }
}

/// Returns `true` when `name` is one of the lava DMA names
/// (`LAVA1`-`LAVA5`).
///
/// Mirrors the `LAVA1`-`LAVA5` entries in the Java reference's
/// `background_manager_mapping.properties`.
fn is_lava_name(name: &str) -> bool {
    matches!(name, "LAVA1" | "LAVA2" | "LAVA3" | "LAVA4" | "LAVA5")
}

/// Returns `true` when `name` is one of the killing-water DMA names.
///
/// The Java reference enumerates each `WATER*` variant explicitly
/// (`WATERTL`, `WATERTR`, `WATERRD`, `WATERLD`, `WATERML`, `WATERMR` and
/// their `*2`-`*4` animation siblings) in
/// `background_manager_mapping.properties`.  The Rust port keeps the same
/// surface area by checking the `WATER` prefix and one of the
/// orientation-tagged suffixes; the trailing animation index (`2`, `3`, `4`)
/// is optional.
fn is_water_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("WATER") else {
        return false;
    };
    let prefix = match rest.len() {
        2 => rest,
        3 => &rest[..2],
        _ => return false,
    };
    if !matches!(prefix, "TL" | "TR" | "RD" | "LD" | "ML" | "MR") {
        return false;
    }
    if rest.len() == 3 {
        matches!(rest.as_bytes()[2], b'2' | b'3' | b'4')
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BaseTreeBackground, FFloorBackground, FroofBackground, Kill2Background, KillLavaBackground,
        KillWaterBackground, SpikeBackground, is_lava_name, is_water_name,
    };
    use crate::asset_cache::AssetCache;
    use crate::entities::objects::player::PlayerEntity;
    use openjill_core::{
        BackgroundEntity, DeathKind, MessageDispatcher, MessageHandler, MessagePayload, MessageType,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Object record byte length used by the synthetic JN buffer helpers.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Shared recording buffer used by [`RecordingHandler`] for hazard tests.
    type RecordingBuffer = Arc<Mutex<Vec<(MessageType, MessagePayload)>>>;

    /// Test helper: records every delivered message into a shared buffer so
    /// the test can verify which messages a hazard cell dispatched.
    struct RecordingHandler(RecordingBuffer);

    impl MessageHandler for RecordingHandler {
        /// Appends `(msg_type, payload)` to the recording buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Builds a [`PlayerEntity`] at `(x, y)` for hazard touch tests.
    ///
    /// The player carries a 16x16 bounding box because the synthetic JN
    /// record leaves width/height zero and the `PlayerEntity::new` fallback
    /// substitutes `BLOCK_SIZE_I`.
    fn make_player_at(x: i32, y: i32) -> PlayerEntity {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let cache = AssetCache::synthetic();
        PlayerEntity::new(&jn.objects()[0], &cache)
    }

    /// Builds a dispatcher with a [`RecordingHandler`] subscribed to
    /// [`MessageType::DieRestartLevel`] and returns it along with the shared
    /// buffer the test asserts against.
    fn dispatcher_with_die_recorder() -> (MessageDispatcher, RecordingBuffer) {
        let buffer: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::DieRestartLevel,
            Box::new(RecordingHandler(Arc::clone(&buffer))),
        );
        (dispatcher, buffer)
    }

    /// Unit under test: [`is_lava_name`].
    #[test]
    fn is_lava_name_matches_lava1_through_lava5() {
        for name in ["LAVA1", "LAVA2", "LAVA3", "LAVA4", "LAVA5"] {
            assert!(is_lava_name(name), "{name} must be classified as lava");
        }
        assert!(!is_lava_name("LAVA"));
        assert!(!is_lava_name("LAVA6"));
        assert!(!is_lava_name("WATERTL"));
    }

    /// Unit under test: [`is_water_name`].
    #[test]
    fn is_water_name_accepts_all_orientation_and_animation_variants() {
        for prefix in ["TL", "TR", "RD", "LD", "ML", "MR"] {
            let base = format!("WATER{prefix}");
            assert!(is_water_name(&base), "{base} must be classified as water");
            for digit in ['2', '3', '4'] {
                let animated = format!("WATER{prefix}{digit}");
                assert!(
                    is_water_name(&animated),
                    "{animated} must be classified as water"
                );
            }
        }
        assert!(!is_water_name("WATERTL5"));
        assert!(!is_water_name("WATER"));
        assert!(!is_water_name("BASETREE"));
    }

    /// Unit under test: [`KillLavaBackground::on_player_touch`].
    ///
    /// Preconditions: a lava cell built from a zero-DMA `map_code` so the
    /// inner cell is transparent; a dispatcher subscribed to
    /// [`MessageType::DieRestartLevel`] so the test can verify the cell
    /// does **not** send it directly; a fresh player.
    ///
    /// Invariants asserted: the player's recorded death classification is
    /// [`DeathKind::OtherBackground`]; the cell does not dispatch
    /// `DieRestartLevel` directly (the player's `Die` sub-state drives that
    /// after the die animation completes, so the level transition overlay
    /// waits for the death animation instead of starting on the touch frame).
    #[test]
    fn kill_lava_on_player_touch_arms_other_background_kill() {
        let cache = AssetCache::synthetic();
        let mut cell = KillLavaBackground::for_map_code(0, &cache);
        let mut player = make_player_at(0, 0);
        let (mut dispatcher, buffer) = dispatcher_with_die_recorder();

        cell.on_player_touch(&mut player, &mut dispatcher);

        assert!(
            buffer.lock().unwrap().is_empty(),
            "lava on_player_touch must not dispatch DieRestartLevel itself"
        );
        assert_eq!(
            player.death_kind(),
            Some(DeathKind::OtherBackground),
            "lava death must classify as OtherBackground"
        );
    }

    /// Unit under test: [`KillWaterBackground::on_player_touch`].
    ///
    /// Invariants asserted: classifies the player's death as
    /// [`DeathKind::Water`] and does not dispatch `DieRestartLevel` itself.
    #[test]
    fn kill_water_on_player_touch_arms_water_kill() {
        let cache = AssetCache::synthetic();
        let mut cell = KillWaterBackground::for_map_code(0, &cache);
        let mut player = make_player_at(0, 0);
        let (mut dispatcher, buffer) = dispatcher_with_die_recorder();

        cell.on_player_touch(&mut player, &mut dispatcher);

        assert!(buffer.lock().unwrap().is_empty());
        assert_eq!(player.death_kind(), Some(DeathKind::Water));
    }

    /// Unit under test: [`SpikeBackground::on_player_touch`].
    ///
    /// Invariants asserted: player records the `OtherBackground` death
    /// classification; the cell does not dispatch `DieRestartLevel` itself.
    #[test]
    fn spike_on_player_touch_arms_other_background_kill() {
        let cache = AssetCache::synthetic();
        let mut cell = SpikeBackground::for_map_code(0, &cache);
        let mut player = make_player_at(0, 0);
        let (mut dispatcher, buffer) = dispatcher_with_die_recorder();

        cell.on_player_touch(&mut player, &mut dispatcher);

        assert!(buffer.lock().unwrap().is_empty());
        assert_eq!(player.death_kind(), Some(DeathKind::OtherBackground));
    }

    /// Unit under test: [`Kill2Background::on_player_touch`].
    ///
    /// Invariants asserted: player records the `OtherBackground` death
    /// classification, distinguishing the thorn/stalactite hazards from water
    /// but matching the lava family; cell does not dispatch `DieRestartLevel`
    /// itself.
    #[test]
    fn kill_two_on_player_touch_arms_other_background_kill() {
        let cache = AssetCache::synthetic();
        let mut cell = Kill2Background::for_map_code(0, &cache);
        let mut player = make_player_at(0, 0);
        let (mut dispatcher, buffer) = dispatcher_with_die_recorder();

        cell.on_player_touch(&mut player, &mut dispatcher);

        assert!(buffer.lock().unwrap().is_empty());
        assert_eq!(player.death_kind(), Some(DeathKind::OtherBackground));
    }

    /// Unit under test: [`BaseTreeBackground::is_climbable`].
    ///
    /// Invariants asserted: the climbable flag is `true`, which is the only
    /// signal the player entity consults to enter the climbing state.
    #[test]
    fn base_tree_reports_is_climbable() {
        let cache = AssetCache::synthetic();
        let cell = BaseTreeBackground::for_map_code(0, &cache);
        assert!(cell.is_climbable(), "BASETREE cells must be climbable");
        assert!(
            cell.is_passthrough(),
            "BASETREE cells must allow the player to walk through them"
        );
    }

    /// Unit under test: [`FFloorBackground::is_passthrough`].
    ///
    /// Invariants asserted: the passthrough flag is `true` so the player
    /// falls through the cell from above.
    #[test]
    fn ffloor_reports_is_passthrough() {
        let cache = AssetCache::synthetic();
        let cell = FFloorBackground::for_map_code(0, &cache);
        assert!(
            cell.is_passthrough(),
            "FFLOOR cells must be passable so the player can drop through"
        );
    }

    /// Unit under test: [`FroofBackground::is_passthrough`].
    ///
    /// Invariants asserted: pass-through ceilings report passable so the
    /// player can rise through them.
    #[test]
    fn froof_reports_is_passthrough() {
        let cache = AssetCache::synthetic();
        let cell = FroofBackground::for_map_code(0, &cache);
        assert!(
            cell.is_passthrough(),
            "FROOF cells must be passable so the player can jump through them"
        );
    }

    /// Unit under test: [`FFloorBackground::blocks_vertical`].
    ///
    /// Invariants asserted: `FFLOOR` blocks the player only while rising
    /// (`player_yd < 0`); a falling or stationary player passes through so the
    /// "fall-through floor" half of the original behaviour stays intact.
    #[test]
    fn ffloor_blocks_only_upward_motion() {
        let cache = AssetCache::synthetic();
        let cell = FFloorBackground::for_map_code(0, &cache);
        assert!(
            cell.blocks_vertical(-1),
            "FFLOOR must block a rising player so the cell behaves like a floor from below"
        );
        assert!(
            !cell.blocks_vertical(1),
            "FFLOOR must let a falling player drop through"
        );
        assert!(
            !cell.blocks_vertical(0),
            "FFLOOR must let a stationary player rest on existing floor below"
        );
    }

    /// Unit under test: [`FroofBackground::blocks_vertical`].
    ///
    /// Invariants asserted: `FROOF` blocks the player only while falling
    /// (`player_yd > 0`); a rising or stationary player passes through so the
    /// "jump up through roof" half of the original behaviour stays intact.
    #[test]
    fn froof_blocks_only_downward_motion() {
        let cache = AssetCache::synthetic();
        let cell = FroofBackground::for_map_code(0, &cache);
        assert!(
            cell.blocks_vertical(1),
            "FROOF must catch a falling player so the cell behaves like a floor"
        );
        assert!(
            !cell.blocks_vertical(-1),
            "FROOF must let a rising player jump through"
        );
    }
}
