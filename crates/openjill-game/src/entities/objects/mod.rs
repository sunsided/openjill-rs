//! Object entity implementations registered against JN object type ids.
//!
//! Every concrete type listed in `objects_manager_mapping.json` of the Java
//! reference is expected to eventually live as one submodule below.  The
//! registry covers the player, pickups, keys, doors, triggers, checkpoints,
//! projectiles, switches, lifts, and decoration entities required by epic 6.
//! Three low-priority types are explicitly stubbed (types 40, 49, 67); all
//! remaining unknown types fall through to the catch-all [`stub::StubObjectEntity`].

pub mod apple;
pub mod bees;
pub mod blade;
pub mod bonus;
pub mod bubbles;
pub mod bullet;
pub mod checkpoint;
pub mod collapsing_ceiling;
pub mod crab;
pub mod enemy_shared;
pub mod eyes;
pub mod falling_spike;
pub mod firebird_enemy;
pub mod firebird_player;
pub mod firebird_weapon;
pub mod flame;
pub mod frog;
pub mod gator;
pub mod ghost;
pub mod giant_ant;
pub mod hit_fire;
pub mod hive;
pub mod huge_letter_tile;
pub mod knife;
pub mod lift;
pub mod lock_door;
pub mod player;
pub mod point;
pub mod red_key;
pub mod rock_key;
pub mod rolling_rock;
pub mod scatter_particle;
pub mod skull;
pub mod snake;
pub mod spark;
pub mod stub;
pub mod switch;
pub mod text_tile;
pub mod toggle_wall;
pub mod touch_trigger;

use openjill_core::ObjectEntity;
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

pub use apple::AppleEntity;
pub use bees::BeesEntity;
pub use blade::BladeEntity;
pub use bonus::BonusEntity;
pub use bubbles::BubblesEntity;
pub use bullet::BulletEntity;
pub use checkpoint::CheckPointEntity;
pub use collapsing_ceiling::CollapsingCeilingEntity;
pub use crab::CrabEntity;
pub use eyes::EyesEntity;
pub use falling_spike::FallingSpikeEntity;
pub use firebird_enemy::FirebirdEnemyEntity;
pub use firebird_player::FirebirdPlayerEntity;
pub use firebird_weapon::FirebirdWeaponEntity;
pub use flame::FlameEntity;
pub use frog::FrogEntity;
pub use gator::GatorEntity;
pub use ghost::GhostEntity;
pub use giant_ant::GiantAntEntity;
pub use hit_fire::HitFireEntity;
pub use hive::HiveEntity;
pub use huge_letter_tile::HugeLetterTileEntity;
pub use knife::KnifeEntity;
pub use lift::LiftEntity;
pub use lock_door::LockedDoorEntity;
pub use player::PlayerEntity;
pub use point::PointEntity;
pub use red_key::RedKeyEntity;
pub use rock_key::RockKeyEntity;
pub use rolling_rock::RollingRockEntity;
pub use scatter_particle::ScatterParticleEntity;
pub use skull::SkullEntity;
pub use snake::SnakeEntity;
pub use spark::SparkEntity;
pub use stub::StubObjectEntity;
pub use switch::SwitchEntity;
pub use text_tile::TextTileEntity;
pub use toggle_wall::ToggleWallEntity;
pub use touch_trigger::TouchTriggerEntity;

/// Builds the correct [`ObjectEntity`] implementation for a JN object record.
///
/// `type_id` is the raw object type byte from the JN object list.  `item`
/// supplies the position, dimensions, and per-object metadata required by the
/// individual entity types.  `string_entry` is the JN string-stack value the
/// object's `string_index` points at (when present), forwarded to entities
/// like [`CheckPointEntity`] whose Java reference reads music-and-map flags
/// from the same string.  `cache` supplies shared episode assets that some
/// entities consult at construction time.
///
/// Returns [`StubObjectEntity`] for any `type_id` that does not have a
/// registered implementation; the stub logs the missing type once and is then
/// otherwise inert, so absent managers cannot crash the level loop.
pub fn make_object_entity(
    type_id: u8,
    item: &JnObject,
    string_entry: Option<&str>,
    cache: &AssetCache,
) -> Box<dyn ObjectEntity> {
    match type_id {
        0 => Box::new(PlayerEntity::new(item, cache)),
        1 => Box::new(AppleEntity::new(item, cache)),
        2 => Box::new(KnifeEntity::new(item, cache)),
        12 => Box::new(CheckPointEntity::new(item, string_entry, cache)),
        14 => Box::new(RedKeyEntity::new(item, cache)),
        15 => Box::new(TouchTriggerEntity::new(item, cache)),
        20 | 21 => Box::new(TextTileEntity::new(item, cache)),
        22 => Box::new(FrogEntity::new(item, cache)),
        24 => Box::new(LockedDoorEntity::new(item, cache)),
        25 => Box::new(CollapsingCeilingEntity::new(item, cache)),
        26 => Box::new(ToggleWallEntity::new(item, cache)),
        27 => Box::new(PointEntity::new(item, cache)),
        28 => Box::new(BonusEntity::new(item, cache)),
        29 => Box::new(GiantAntEntity::new(item, cache)),
        30 => Box::new(FirebirdEnemyEntity::new(item, cache)),
        31 => Box::new(FlameEntity::new(item, cache)),
        32 => Box::new(SwitchEntity::new(item, cache)),
        33 => Box::new(RockKeyEntity::new(item, cache)),
        35 => Box::new(RollingRockEntity::new(item, cache)),
        36 => Box::new(BulletEntity::new(item, cache)),
        37 => Box::new(HitFireEntity::new(item, cache)),
        38 => Box::new(FallingSpikeEntity::new(item, cache)),
        39 => Box::new(SnakeEntity::new(item, cache)),
        40 => Box::new(StubObjectEntity::silent(type_id, item)),
        42 => Box::new(HugeLetterTileEntity::new(item, cache)),
        45 => Box::new(HiveEntity::new(item, cache)),
        46 => Box::new(BeesEntity::new(item, cache)),
        47 => Box::new(CrabEntity::new(item, cache)),
        48 => Box::new(GatorEntity::new(item, cache)),
        49 => Box::new(StubObjectEntity::silent(type_id, item)),
        50 => Box::new(BladeEntity::new(item, cache)),
        51 => Box::new(SkullEntity::new(item, cache)),
        53 => Box::new(GhostEntity::new(item, cache)),
        56 => Box::new(FirebirdPlayerEntity::new(item, cache)),
        58 => Box::new(BubblesEntity::new(item, cache)),
        61 => Box::new(LiftEntity::new(item, cache)),
        62 => Box::new(FirebirdWeaponEntity::new(item, cache)),
        64 => Box::new(EyesEntity::new(item, cache)),
        65 => Box::new(SparkEntity::new(item, cache)),
        67 => Box::new(StubObjectEntity::silent(type_id, item)),
        other => Box::new(StubObjectEntity::new(other, item)),
    }
}

#[cfg(test)]
mod tests {
    use super::make_object_entity;
    use crate::asset_cache::AssetCache;
    use openjill_data::jn::JnFile;

    /// Object record size in bytes; mirrors the constant used by the level
    /// screen tests.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a JN file with one zero-initialised object whose `object_type`
    /// is set to `type_id`, and returns that object's `JnObject`.
    fn synthetic_object_of_type(type_id: u8) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = type_id;
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: `make_object_entity` for an unregistered type id.
    ///
    /// Preconditions: a synthetic JN object record carries `object_type = 99`,
    /// which has no manager in the registered factory table.
    ///
    /// Invariants asserted: construction does not panic, the returned entity
    /// reports neither `is_player` nor `is_checkpoint`, and its bounding box
    /// is the zero-sized rect that the `StubObjectEntity` produces.
    #[test]
    fn make_object_entity_returns_stub_for_unregistered_type() {
        let cache = AssetCache::synthetic();
        let item = synthetic_object_of_type(99);
        let entity = make_object_entity(99, &item, None, &cache);
        assert!(!entity.is_player());
        assert!(!entity.is_checkpoint());
        let bbox = entity.bounding_box();
        assert_eq!(bbox.w, 0);
        assert_eq!(bbox.h, 0);
    }

    /// Unit under test: `make_object_entity` for the player type id.
    ///
    /// Preconditions: a synthetic JN object record carries `object_type = 0`.
    ///
    /// Invariants asserted: the returned entity reports `is_player`, mirroring
    /// the `PlayerEntity` placeholder installed for type 0.
    #[test]
    fn make_object_entity_returns_player_for_type_zero() {
        let cache = AssetCache::synthetic();
        let item = synthetic_object_of_type(0);
        let entity = make_object_entity(0, &item, None, &cache);
        assert!(entity.is_player());
        assert!(!entity.is_checkpoint());
    }

    /// Unit under test: `make_object_entity` for the checkpoint type id.
    ///
    /// Preconditions: a synthetic JN object record carries `object_type = 12`.
    ///
    /// Invariants asserted: the returned entity reports `is_checkpoint`,
    /// mirroring the `CheckPointEntity` placeholder installed for type 12.
    #[test]
    fn make_object_entity_returns_checkpoint_for_type_twelve() {
        let cache = AssetCache::synthetic();
        let item = synthetic_object_of_type(12);
        let entity = make_object_entity(12, &item, None, &cache);
        assert!(entity.is_checkpoint());
        assert!(!entity.is_player());
    }

    /// Unit under test: `make_object_entity` for the three explicitly-stubbed
    /// low-priority types (40 `UnderWaterRockEntity`, 49 `EpicEntity`, 67
    /// `DemoMapEntity`).
    ///
    /// Preconditions: synthetic JN object records carry each of the three
    /// type ids.
    ///
    /// Invariants asserted: construction does not panic; the returned entity
    /// reports neither `is_player` nor `is_checkpoint`; bounding box is
    /// zero-sized (matching `StubObjectEntity` output).
    #[test]
    fn make_object_entity_returns_stub_for_explicitly_stubbed_types() {
        let cache = AssetCache::synthetic();
        for type_id in [40u8, 49, 67] {
            let item = synthetic_object_of_type(type_id);
            let entity = make_object_entity(type_id, &item, None, &cache);
            assert!(!entity.is_player(), "type {type_id} should not be player");
            assert!(
                !entity.is_checkpoint(),
                "type {type_id} should not be checkpoint"
            );
            let bbox = entity.bounding_box();
            assert_eq!(bbox.w, 0, "type {type_id} stub bbox width");
            assert_eq!(bbox.h, 0, "type {type_id} stub bbox height");
        }
    }

    /// Unit under test: `make_object_entity` for the apple type id.
    ///
    /// Preconditions: a synthetic JN object record carries `object_type = 1`.
    ///
    /// Invariants asserted: the returned entity reports neither `is_player`
    /// nor `is_checkpoint`, and its bounding box matches a single 16x16 block
    /// (the placeholder fallback for zero-dimensions JN records).
    #[test]
    fn make_object_entity_returns_apple_for_type_one() {
        let cache = AssetCache::synthetic();
        let item = synthetic_object_of_type(1);
        let entity = make_object_entity(1, &item, None, &cache);
        assert!(!entity.is_player());
        assert!(!entity.is_checkpoint());
        let bbox = entity.bounding_box();
        assert_eq!(bbox.w, 16);
        assert_eq!(bbox.h, 16);
    }
}
