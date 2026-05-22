//! Background entity implementations registered against DMA cell names.
//!
//! Issue 57 only ships the catch-all [`standard::StdBackgroundEntity`] used
//! for every DMA cell.  Named entries from `background_manager_mapping.properties`
//! (`MAPDOOR`, `BASETREE`, `BASEWATER`, hazard cells, etc.) land in later
//! child issues of epic 6.

pub mod standard;

use openjill_core::BackgroundEntity;

use crate::asset_cache::AssetCache;

pub use standard::StdBackgroundEntity;

/// Builds the correct [`BackgroundEntity`] implementation for a DMA cell.
///
/// `dma_name` is the trimmed DMA name string (uppercase ASCII, e.g.
/// `"BASETREE"`).  `map_code` is the masked background map code that selected
/// the DMA entry, used so a specialised entity that needs to round-trip back
/// through `DmaFile::get_by_map_code` can do so.  `cache` supplies shared
/// episode assets (DMA, SHA) required to construct individual entities.
///
/// Currently delegates to [`StdBackgroundEntity::for_map_code`] for every
/// name.  Specialised entities will branch on `dma_name` in subsequent child
/// issues.
pub fn make_background_entity(
    dma_name: &str,
    map_code: u16,
    cache: &AssetCache,
) -> Box<dyn BackgroundEntity> {
    let _ = dma_name;
    Box::new(StdBackgroundEntity::for_map_code(map_code, cache))
}
