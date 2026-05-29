//! Shared background-collision helpers used by all enemy patrol entities.
//!
//! These are direct translations of `UtilityObjectEntity.giveBlockAtRight`,
//! `giveBlockAtLeft`, and `checkIfFloorUnderObject` from the Java reference.

use openjill_core::{BackgroundEntity, BackgroundGrid, layout::BLOCK_SIZE_I};

use crate::asset_cache::AssetCache;

/// Tiny dependency-free xorshift32 PRNG for enemy behaviour that mirrors the
/// Java reference's `Math.random()` calls (bee speed ranges, crab climb
/// trigger).
///
/// Seeded per entity (typically from its spawn coordinates) so behaviour is
/// reproducible in tests while still varying between instances, standing in
/// for Java's global `Math.random()`.
pub(crate) struct EnemyRng {
    state: u32,
}

impl EnemyRng {
    /// Creates an RNG from `seed` (forced non-zero so xorshift never sticks
    /// at zero).
    pub(crate) fn new(seed: u32) -> Self {
        Self {
            state: seed | 0x9E37_79B9,
        }
    }

    fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// Uniform integer in `[lo, hi)`, mirroring Java
    /// `(int)(Math.random() * (hi - lo)) + lo`.  Returns `lo` when the range
    /// is empty.
    pub(crate) fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % ((hi - lo) as u32)) as i32
    }
}

/// Returns the pixel dimensions `(w, h)` of the first tile in the SHA
/// tileset identified by `tileset_index`.
///
/// Thin wrapper around [`AssetCache::tile_dims`] kept for source-stability
/// of existing enemy entity constructors; new call sites should prefer the
/// `AssetCache` method directly.
pub(crate) fn sprite_dims(cache: &AssetCache, tileset_index: u8) -> (i32, i32) {
    cache.tile_dims(tileset_index)
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

/// `true` when the entity can grab a vine at this position.
///
/// Mirror of `UtilityObjectEntity.isClimbing`: the grab only engages when the
/// entity's X is aligned to a block column (`x % blockSize == 0`); then any
/// climbable (`isVine`) cell spanned by the bounding box counts.
pub(crate) fn is_on_vine(backgrounds: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    if x.rem_euclid(BLOCK_SIZE_I) != 0 {
        return false;
    }
    let start_x = x.div_euclid(BLOCK_SIZE_I);
    let end_x = (x + w - 1).div_euclid(BLOCK_SIZE_I);
    let start_y = y.div_euclid(BLOCK_SIZE_I);
    let end_y = (y + h - 1).div_euclid(BLOCK_SIZE_I);
    for cx in start_x..=end_x {
        for cy in start_y..=end_y {
            if cx < 0 || cy < 0 {
                continue;
            }
            if backgrounds
                .get(cx as usize, cy as usize)
                .map(BackgroundEntity::is_climbable)
                .unwrap_or(false)
            {
                return true;
            }
        }
    }
    false
}

/// Slides the bounding box horizontally toward `dx`, one pixel at a time,
/// stopping flush against a solid (non-passthrough) cell or the map edge.
///
/// Mirrors `UtilityObjectEntity.moveObjectLeft`/`moveObjectRight`: a partial
/// move that snaps to the wall instead of refusing the whole step.  Returns
/// the signed number of pixels actually moved (`0` when fully blocked).
pub(crate) fn slide_x(
    backgrounds: &BackgroundGrid,
    x: &mut i32,
    y: i32,
    w: i32,
    h: i32,
    dx: i32,
) -> i32 {
    let step = dx.signum();
    let max_x = (backgrounds.width as i32) * BLOCK_SIZE_I - w;
    let cy_top = y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cy_bot = (y + h - 1)
        .div_euclid(BLOCK_SIZE_I)
        .max(0)
        .min((backgrounds.height as i32) - 1) as usize;
    let mut moved = 0;
    for _ in 0..dx.abs() {
        let nx = *x + step;
        if nx < 0 || nx > max_x {
            break;
        }
        let probe_col = if step > 0 { nx + w - 1 } else { nx };
        let cell_x = probe_col.div_euclid(BLOCK_SIZE_I);
        if cell_x < 0 || cell_x as usize >= backgrounds.width {
            break;
        }
        let blocked = (cy_top..=cy_bot).any(|cy| {
            backgrounds
                .get(cell_x as usize, cy)
                .map(|c| !c.is_passthrough())
                .unwrap_or(false)
        });
        if blocked {
            break;
        }
        *x = nx;
        moved += step;
    }
    moved
}

/// Slides the bounding box vertically toward `dy`, one pixel at a time,
/// stopping flush against a cell that blocks vertical motion in that direction
/// or the map edge.
///
/// Mirrors `UtilityObjectEntity.moveObjectUp`/`moveObjectDown`.  Returns the
/// signed number of pixels actually moved (`0` when fully blocked, which the
/// caller reads as "hit ceiling" when rising or "landed" when falling).
pub(crate) fn slide_y(
    backgrounds: &BackgroundGrid,
    x: i32,
    y: &mut i32,
    w: i32,
    h: i32,
    dy: i32,
) -> i32 {
    let step = dy.signum();
    let max_y = (backgrounds.height as i32) * BLOCK_SIZE_I - h;
    let cx_left = x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cx_right = (x + w - 1)
        .div_euclid(BLOCK_SIZE_I)
        .max(0)
        .min((backgrounds.width as i32) - 1) as usize;
    let mut moved = 0;
    for _ in 0..dy.abs() {
        let ny = *y + step;
        if ny < 0 || ny > max_y {
            break;
        }
        let probe_row = if step > 0 { ny + h - 1 } else { ny };
        let cell_y = probe_row.div_euclid(BLOCK_SIZE_I);
        if cell_y < 0 || cell_y as usize >= backgrounds.height {
            break;
        }
        let blocked = (cx_left..=cx_right).any(|cx| {
            backgrounds
                .get(cx, cell_y as usize)
                .map(|c| c.blocks_vertical(step))
                .unwrap_or(false)
        });
        if blocked {
            break;
        }
        *y = ny;
        moved += step;
    }
    moved
}
