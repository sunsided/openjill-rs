//! Per-type object and background entity implementations plus the factories
//! that materialise them from JN and DMA data.
//!
//! The factories are the single entry point used by `LevelScreen` to turn
//! parsed JN objects and DMA-named background cells into the trait-object
//! values it iterates each tick.  Unrecognised object types fall through to
//! [`objects::stub::StubObjectEntity`], which logs the missing type once and
//! otherwise behaves as a no-op so missing managers cannot crash the level
//! loop.  Unrecognised DMA names fall through to
//! [`backgrounds::standard::StdBackgroundEntity`], the solid/passthrough cell
//! handler that mirrors the Java reference's default background manager.

pub mod backgrounds;
pub mod objects;

use openjill_core::{BackgroundEntity, ObjectEntity};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

pub use backgrounds::make_background_entity;
pub use objects::make_object_entity;

/// Builds the correct [`ObjectEntity`] implementation for a JN object record.
///
/// Delegates to [`objects::make_object_entity`]; re-exported at the crate
/// surface so external callers can construct entities without descending into
/// the per-type module hierarchy.  See [`objects::make_object_entity`] for the
/// registered type table.
pub fn make_object_entity_for(
    type_id: u8,
    item: &JnObject,
    string_entry: Option<&str>,
    cache: &AssetCache,
) -> Box<dyn ObjectEntity> {
    make_object_entity(type_id, item, string_entry, cache)
}

/// Builds the correct [`BackgroundEntity`] implementation for a DMA cell.
///
/// Delegates to [`backgrounds::make_background_entity`]; re-exported at the
/// crate surface so external callers can construct background cells without
/// descending into the per-name module hierarchy.
pub fn make_background_entity_for(
    dma_name: &str,
    map_code: u16,
    cache: &AssetCache,
) -> Box<dyn BackgroundEntity> {
    make_background_entity(dma_name, map_code, cache)
}
