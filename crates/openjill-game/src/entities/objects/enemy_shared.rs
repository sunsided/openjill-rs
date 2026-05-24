//! Shared background-collision helpers used by all enemy patrol entities.
//!
//! These are direct translations of `UtilityObjectEntity.giveBlockAtRight`,
//! `giveBlockAtLeft`, and `checkIfFloorUnderObject` from the Java reference.

use openjill_core::{BackgroundGrid, layout::BLOCK_SIZE_I};

use crate::asset_cache::AssetCache;

/// Returns the pixel dimensions `(w, h)` of the first tile in the SHA
/// tileset identified by `tileset_index` (matched against
/// `ShaTileSet::entry_index`).  Falls back to `(BLOCK_SIZE_I, BLOCK_SIZE_I)`
/// when the tileset or its tile list is absent (e.g. synthetic test cache).
pub(crate) fn sprite_dims(cache: &AssetCache, tileset_index: u8) -> (i32, i32) {
    cache
        .sha
        .tilesets()
        .iter()
        .find(|ts| ts.entry_index() == usize::from(tileset_index))
        .and_then(|ts| ts.tiles().first())
        .map(|t| (i32::from(t.width()), i32::from(t.height())))
        .unwrap_or((BLOCK_SIZE_I, BLOCK_SIZE_I))
}

/// `true` when the column directly ahead of the entity is occupied by a
/// solid (non-passthrough) cell.  `x_speed` sign selects which side is
/// "ahead".
///
/// Only `!is_passthrough()` is used; `is_stair()` is intentionally excluded
/// because the DMA flag for "not a stair" is an opt-out bit, so nearly every
/// tile (including passthrough shade tiles like BLSHADE) reports `is_stair`
/// true.  Using it as a blocking criterion would treat transparent shade tiles
/// as walls.
pub(crate) fn blocked_ahead(
    backgrounds: &BackgroundGrid,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
) -> bool {
    let probe_x = if x_speed > 0 { x + w } else { x - 1 };
    let cell_x = probe_x.div_euclid(BLOCK_SIZE_I);
    if cell_x < 0 || cell_x as usize >= backgrounds.width {
        return true;
    }
    let cell_x = cell_x as usize;
    let cy_top = y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cy_bot = (y + h - 1)
        .div_euclid(BLOCK_SIZE_I)
        .max(0)
        .min((backgrounds.height as i32) - 1) as usize;
    for cy in cy_top..=cy_bot {
        if let Some(cell) = backgrounds.get(cell_x, cy)
            && !cell.is_passthrough()
        {
            return true;
        }
    }
    false
}

/// `true` when there is a solid cell directly below the entity's next
/// horizontal position.  Returning `false` indicates a gap so the patrol
/// should reverse direction.
pub(crate) fn floor_under_next(
    backgrounds: &BackgroundGrid,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
) -> bool {
    let probe_x = if x_speed > 0 { x + w } else { x - 1 };
    let cell_x = probe_x.div_euclid(BLOCK_SIZE_I);
    if cell_x < 0 || cell_x as usize >= backgrounds.width {
        return false;
    }
    let cell_x = cell_x as usize;
    let cell_y = (y + h).div_euclid(BLOCK_SIZE_I);
    if cell_y < 0 || cell_y as usize >= backgrounds.height {
        return false;
    }
    let cell_y = cell_y as usize;
    backgrounds
        .get(cell_x, cell_y)
        .map(|c| c.blocks_vertical(1))
        .unwrap_or(false)
}

/// `true` when there is a solid cell directly below the entity's current
/// footprint (used to detect landing after a fall or jump).
pub(crate) fn floor_below(backgrounds: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    let cell_y = (y + h).div_euclid(BLOCK_SIZE_I);
    if cell_y < 0 || cell_y as usize >= backgrounds.height {
        return false;
    }
    let cell_y = cell_y as usize;
    let cx_left = x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cx_right = (x + w - 1)
        .div_euclid(BLOCK_SIZE_I)
        .max(0)
        .min((backgrounds.width as i32) - 1) as usize;
    for cx in cx_left..=cx_right {
        if let Some(cell) = backgrounds.get(cx, cell_y)
            && cell.blocks_vertical(1)
        {
            return true;
        }
    }
    false
}
