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

/// Canonical editor-facing name for each implemented JN object type id.
///
/// Single source of truth shared by [`make_object_entity`] (the runtime
/// factory above) and the in-game level editor's add-/identify-object commands
/// (epic #210). Only type ids with a registered entity implementation are
/// named; unimplemented / stubbed ids (e.g. 40, 49, 67) have no canonical name.
/// The names mirror the port's entity types - the Java reference's
/// `objects_manager_mapping.json`, now removed, was the original id-to-name
/// source. The table is ordered by ascending id; a couple of ids share a name
/// (e.g. the two text-tile ids 20/21).
pub const OBJECT_TYPE_NAMES: &[(u8, &str)] = &[
    (0, "Player"),
    (1, "Apple"),
    (2, "Knife"),
    (12, "Checkpoint"),
    (14, "RedKey"),
    (15, "TouchTrigger"),
    (20, "TextTile"),
    (21, "TextTile"),
    (22, "Frog"),
    (24, "LockedDoor"),
    (25, "CollapsingCeiling"),
    (26, "ToggleWall"),
    (27, "Point"),
    (28, "Bonus"),
    (29, "GiantAnt"),
    (30, "Firebird"),
    (31, "Flame"),
    (32, "Switch"),
    (33, "RockKey"),
    (35, "RollingRock"),
    (36, "Bullet"),
    (37, "HitFire"),
    (38, "FallingSpike"),
    (39, "Snake"),
    (42, "HugeLetterTile"),
    (45, "Hive"),
    (46, "Bees"),
    (47, "Crab"),
    (48, "Gator"),
    (50, "Blade"),
    (51, "Skull"),
    (53, "Ghost"),
    (56, "FirebirdPlayer"),
    (58, "Bubbles"),
    (61, "Lift"),
    (62, "FirebirdWeapon"),
    (64, "Eyes"),
    (65, "Spark"),
];

/// Returns the canonical name for `type_id`, or `None` for an unnamed
/// (unimplemented) type. Ids that share a name each resolve to that name.
pub fn object_type_name(type_id: u8) -> Option<&'static str> {
    OBJECT_TYPE_NAMES
        .iter()
        .find_map(|&(id, name)| (id == type_id).then_some(name))
}

/// Returns the type id for `name` (ASCII case-insensitive), or `None` when no
/// implemented type carries that name. When a name maps to several ids the
/// lowest (canonical) id is returned.
pub fn object_type_id(name: &str) -> Option<u8> {
    OBJECT_TYPE_NAMES
        .iter()
        .filter(|&&(_, candidate)| candidate.eq_ignore_ascii_case(name))
        .map(|&(id, _)| id)
        .min()
}

#[cfg(test)]
mod tests {
    use super::make_object_entity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{MessageDispatcher, RuntimeState};
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

    /// Unit under test: [`ObjectEntity::snapshot`] for the pickup / static-tile
    /// entity group.
    ///
    /// Invariant asserted: a live pickup built from a JN record reproduces that
    /// record via `snapshot` (`parse -> new -> snapshot == parse`). Covers apple
    /// (1), red key (14), rock key (33), bonus (28), text tiles (20, 21), and
    /// huge-letter tiles (42).
    #[test]
    fn pickup_entities_snapshot_round_trips_their_source_record() {
        let cache = AssetCache::synthetic();
        for type_id in [1u8, 14, 33, 28, 20, 21, 42] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(64, 80);
            let entity = make_object_entity(type_id, &item, None, &cache);
            assert_eq!(
                entity.snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip its source record"
            );
        }
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the floating score popup
    /// (type 27), whose live position, speeds, and remaining lifetime must
    /// round-trip while the popup is still alive.
    #[test]
    fn point_entity_snapshot_round_trips_while_alive() {
        let cache = AssetCache::synthetic();
        let mut item = synthetic_object_of_type(27);
        item.set_position(40, 50);
        item.set_speed(2, -3);
        item.set_counter(30);
        let entity = make_object_entity(27, &item, None, &cache);
        assert_eq!(entity.snapshot(), Some(item.clone()));
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the ground-walker enemy
    /// group - crab (47), gator (48), giant ant (29).
    ///
    /// Each record carries a live (non-zero) walk direction, animation
    /// `counter`, `zap_hold`, and a sub-sprite height that exercises the
    /// construction y-adjustment reversal; `snapshot` must reproduce it.
    #[test]
    fn walker_enemy_snapshot_round_trips_with_live_direction() {
        let cache = AssetCache::synthetic();
        for type_id in [47u8, 48, 29] {
            // A live leftward direction (non-zero), and an authored zero speed
            // that new() defaults to the patrol speed; both must round-trip.
            for x_speed in [-4i16, 0] {
                let mut item = synthetic_object_of_type(type_id);
                item.set_position(80, 96);
                item.set_speed(x_speed, 0);
                item.set_counter(2);
                item.set_zap_hold(1);
                // Sub-sprite height exercises the y-adjustment reversal.
                item.set_dimensions(16, 4);
                let entity = make_object_entity(type_id, &item, None, &cache);
                assert_eq!(
                    entity.snapshot(),
                    Some(item.clone()),
                    "type {type_id} (x_speed {x_speed}) snapshot must round-trip"
                );
            }
        }
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the mover-enemy group -
    /// snake (39), ghost (53), firebird enemy (30) - each with its own live
    /// state (snake body width, ghost glide velocity, firebird direction).
    #[test]
    fn mover_enemy_snapshot_round_trips() {
        let cache = AssetCache::synthetic();

        // Snake: live body width + slither direction + animation counter.
        let mut snake = synthetic_object_of_type(39);
        snake.set_position(48, 64);
        snake.set_dimensions(96, 16); // wide enough that new() does not clamp
        snake.set_speed(-3, 0);
        snake.set_counter(2);
        snake.set_zap_hold(1);
        assert_eq!(
            make_object_entity(39, &snake, None, &cache).snapshot(),
            Some(snake.clone()),
            "snake snapshot must round-trip"
        );

        // Ghost: live glide velocity (non-default so it is not collapsed) plus
        // the speed-magnitude counter.
        let mut ghost = synthetic_object_of_type(53);
        ghost.set_position(80, 80);
        ghost.set_speed(0, -3);
        ghost.set_counter(3);
        ghost.set_zap_hold(1);
        assert_eq!(
            make_object_entity(53, &ghost, None, &cache).snapshot(),
            Some(ghost.clone()),
            "ghost snapshot must round-trip"
        );

        // Firebird enemy: flight direction + animation counter.
        let mut firebird = synthetic_object_of_type(30);
        firebird.set_position(96, 32);
        firebird.set_speed(-4, 0);
        firebird.set_counter(2);
        firebird.set_zap_hold(1);
        assert_eq!(
            make_object_entity(30, &firebird, None, &cache).snapshot(),
            Some(firebird.clone()),
            "firebird-enemy snapshot must round-trip"
        );
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the remaining special
    /// enemies - bees (46), skull (51), eyes (64), spark (65), hive (45).
    #[test]
    fn special_enemy_snapshot_round_trips() {
        let cache = AssetCache::synthetic();

        // Position-only persisters (their behavior re-derives on restore); the
        // counter (skull link id, etc.) is preserved from the origin.
        for type_id in [46u8, 51, 64] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(72, 88);
            item.set_counter(3);
            let entity = make_object_entity(type_id, &item, None, &cache);
            assert_eq!(
                entity.snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip"
            );
        }

        // Spark: live vertical speed (non-default) + animation counter.
        let mut spark = synthetic_object_of_type(65);
        spark.set_position(40, 40);
        spark.set_speed(0, 3);
        spark.set_counter(2);
        spark.set_zap_hold(1);
        assert_eq!(
            make_object_entity(65, &spark, None, &cache).snapshot(),
            Some(spark.clone()),
            "spark snapshot must round-trip"
        );

        // Hive: spawn charge counter.
        let mut hive = synthetic_object_of_type(45);
        hive.set_position(64, 64);
        hive.set_counter(2);
        hive.set_zap_hold(1);
        assert_eq!(
            make_object_entity(45, &hive, None, &cache).snapshot(),
            Some(hive.clone()),
            "hive snapshot must round-trip"
        );
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the static / trigger
    /// hazard group - lock door (24), toggle wall (26), switch (32),
    /// checkpoint (12), touch trigger (15), bubbles (58), hit fire (37).
    ///
    /// Each persists position; the authored `counter` (key / link id /
    /// destination) is preserved from the origin and the trigger/animation
    /// state re-derives on restore.
    #[test]
    fn hazard_entities_snapshot_round_trip_their_source_record() {
        let cache = AssetCache::synthetic();
        for type_id in [24u8, 26, 32, 12, 15, 58, 37] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(56, 72);
            item.set_counter(4);
            let entity = make_object_entity(type_id, &item, None, &cache);
            assert_eq!(
                entity.snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip its source record"
            );
        }
    }

    /// Unit under test: a toggled [`ToggleWallEntity`] persists its
    /// passthrough state across a snapshot (it does not reset to solid).
    #[test]
    fn toggled_wall_snapshot_persists_passthrough_state() {
        let cache = AssetCache::synthetic();
        let mut item = synthetic_object_of_type(26);
        item.set_position(56, 72);
        item.set_counter(7); // trigger link id

        let mut wall = make_object_entity(26, &item, None, &cache);
        wall.receive_trigger(7); // flip solid -> passthrough

        // `state` field 1 == STATE_PASSTHROUGH; restoring it yields a
        // passthrough wall rather than the authored solid default.
        let snapshot = wall.snapshot().expect("a toggle wall always persists");
        assert_eq!(
            snapshot.state(),
            1,
            "toggled-open state must survive a save"
        );
    }

    /// Unit under test: a fired [`CheckPointEntity`] is not persisted (it is
    /// being reaped after dispatching its transition), exercising the
    /// conditional `None` branch shared by the removable hazards.
    #[test]
    fn fired_checkpoint_is_not_persisted() {
        let cache = AssetCache::synthetic();
        let item = synthetic_object_of_type(12);
        let mut checkpoint = make_object_entity(12, &item, None, &cache);

        checkpoint.on_touch(&RuntimeState::new(), &mut MessageDispatcher::new());

        assert_eq!(checkpoint.snapshot(), None, "fired checkpoint is not saved");
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the moving-hazard group -
    /// collapsing ceiling (25), flame (31), falling spike (38), rolling rock
    /// (35), lift (61).
    #[test]
    fn moving_hazard_snapshot_round_trips() {
        let cache = AssetCache::synthetic();

        // Position-only persisters (their motion/animation re-derives).
        for type_id in [25u8, 31] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(48, 32);
            assert_eq!(
                make_object_entity(type_id, &item, None, &cache).snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip"
            );
        }

        // Falling spike: live downward speed.
        let mut spike = synthetic_object_of_type(38);
        spike.set_position(64, 16);
        spike.set_speed(0, 5);
        assert_eq!(
            make_object_entity(38, &spike, None, &cache).snapshot(),
            Some(spike.clone()),
            "falling-spike snapshot must round-trip"
        );

        // Rolling rock: live roll direction. Cover both a non-default
        // direction and the authored zero that new() defaults to rolling right
        // (which snapshot must collapse back to 0).
        for x_speed in [-4i16, 0] {
            let mut rock = synthetic_object_of_type(35);
            rock.set_position(96, 80);
            rock.set_speed(x_speed, 0);
            assert_eq!(
                make_object_entity(35, &rock, None, &cache).snapshot(),
                Some(rock.clone()),
                "rolling-rock (x_speed {x_speed}) snapshot must round-trip"
            );
        }

        // Lift: live velocity along its path.
        let mut lift = synthetic_object_of_type(61);
        lift.set_position(32, 48);
        lift.set_speed(2, -1);
        assert_eq!(
            make_object_entity(61, &lift, None, &cache).snapshot(),
            Some(lift.clone()),
            "lift snapshot must round-trip"
        );
    }

    /// Unit under test: [`ObjectEntity::snapshot`] for the projectile / weapon
    /// group and the catch-all stub.
    #[test]
    fn projectile_and_stub_snapshot_round_trips() {
        let cache = AssetCache::synthetic();

        // Pickups (knife 2, blade 50) and the firebird player form (56):
        // position-only.
        for type_id in [2u8, 50, 56] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(40, 56);
            assert_eq!(
                make_object_entity(type_id, &item, None, &cache).snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip"
            );
        }

        // Velocity-carrying weapons (bullet 36, firebird weapon 62).
        for type_id in [36u8, 62] {
            let mut item = synthetic_object_of_type(type_id);
            item.set_position(72, 24);
            item.set_speed(6, -2);
            assert_eq!(
                make_object_entity(type_id, &item, None, &cache).snapshot(),
                Some(item.clone()),
                "type {type_id} snapshot must round-trip"
            );
        }

        // Unrecognized type: the stub persists its record verbatim.
        let mut unknown = synthetic_object_of_type(99);
        unknown.set_position(88, 16);
        assert_eq!(
            make_object_entity(99, &unknown, None, &cache).snapshot(),
            Some(unknown.clone()),
            "stub must persist an unrecognized object verbatim"
        );
    }

    /// Unit under test: [`super::object_type_name`] over the registry.
    ///
    /// Invariants: implemented ids resolve to their canonical name, ids sharing
    /// a name both resolve to it, and unnamed / stubbed ids return `None`.
    #[test]
    fn object_type_name_resolves_implemented_ids() {
        use super::object_type_name;
        assert_eq!(object_type_name(0), Some("Player"));
        assert_eq!(object_type_name(48), Some("Gator"));
        assert_eq!(object_type_name(20), Some("TextTile"));
        assert_eq!(object_type_name(21), Some("TextTile"));
        assert_eq!(object_type_name(40), None); // explicitly stubbed
        assert_eq!(object_type_name(99), None); // unimplemented
    }

    /// Unit under test: [`super::object_type_id`] (case-insensitive lookup).
    ///
    /// Invariants: names resolve regardless of case, a shared name resolves to
    /// the lowest (canonical) id, and unknown names return `None`.
    #[test]
    fn object_type_id_is_case_insensitive_and_canonical() {
        use super::object_type_id;
        assert_eq!(object_type_id("apple"), Some(1));
        assert_eq!(object_type_id("GATOR"), Some(48));
        assert_eq!(object_type_id("TextTile"), Some(20)); // lowest of 20/21
        assert_eq!(object_type_id("nope"), None);
    }

    /// Unit under test: registry self-consistency.
    ///
    /// Invariants: every `(id, name)` entry round-trips through
    /// [`super::object_type_name`], and the table's ids are strictly ascending
    /// (so unique), guarding future edits against typos and duplicates.
    #[test]
    fn object_type_names_are_self_consistent() {
        use super::{OBJECT_TYPE_NAMES, object_type_name};
        for &(id, name) in OBJECT_TYPE_NAMES {
            assert_eq!(object_type_name(id), Some(name), "id {id} must name {name}");
        }
        for window in OBJECT_TYPE_NAMES.windows(2) {
            assert!(
                window[0].0 < window[1].0,
                "object type ids must be strictly ascending (unique)"
            );
        }
    }
}
