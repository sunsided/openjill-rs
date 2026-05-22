//! Object entity implementations registered against JN object type ids.
//!
//! Every concrete type listed in `objects_manager_mapping.json` of the Java
//! reference is expected to eventually live as one submodule below.  Issue 57
//! seeds the registry with the three types required for the per-tick update
//! loop (player, apple, checkpoint) plus the catch-all
//! [`stub::StubObjectEntity`] that the factory returns for unregistered type
//! ids.

pub mod apple;
pub mod checkpoint;
pub mod player;
pub mod stub;

use openjill_core::ObjectEntity;
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

pub use apple::AppleEntity;
pub use checkpoint::CheckPointEntity;
pub use player::PlayerEntity;
pub use stub::StubObjectEntity;

/// Builds the correct [`ObjectEntity`] implementation for a JN object record.
///
/// `type_id` is the raw object type byte from the JN object list.  `item`
/// supplies the position, dimensions, and per-object metadata required by the
/// individual entity types.  `cache` supplies shared episode assets that some
/// entities consult at construction time.
///
/// Returns [`StubObjectEntity`] for any `type_id` that does not have a
/// registered implementation; the stub logs the missing type once and is then
/// otherwise inert, so absent managers cannot crash the level loop.
pub fn make_object_entity(
    type_id: u8,
    item: &JnObject,
    cache: &AssetCache,
) -> Box<dyn ObjectEntity> {
    match type_id {
        0 => Box::new(PlayerEntity::new(item, cache)),
        1 => Box::new(AppleEntity::new(item, cache)),
        12 => Box::new(CheckPointEntity::new(item, cache)),
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
        let entity = make_object_entity(99, &item, &cache);
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
        let entity = make_object_entity(0, &item, &cache);
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
        let entity = make_object_entity(12, &item, &cache);
        assert!(entity.is_checkpoint());
        assert!(!entity.is_player());
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
        let entity = make_object_entity(1, &item, &cache);
        assert!(!entity.is_player());
        assert!(!entity.is_checkpoint());
        let bbox = entity.bounding_box();
        assert_eq!(bbox.w, 16);
        assert_eq!(bbox.h, 16);
    }
}
