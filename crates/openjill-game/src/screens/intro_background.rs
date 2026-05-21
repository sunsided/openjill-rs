//! Shared helper for rendering an INTRO.JN1 background viewport into the game area.

use crate::status_bar::game_area_blit;
use openjill_core::layout::{BLOCK_SIZE, GAME_AREA_H, GAME_AREA_W};
use openjill_core::RenderCommand;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;

/// Tile size in pixels as a signed integer for arithmetic convenience.
const BLOCK_SIZE_I: i32 = BLOCK_SIZE as i32;

/// Renders an INTRO.JN1 background viewport into the game area.
///
/// `offset_x` and `offset_y` follow OpenJill sign convention: a **negative**
/// value shifts the source image left/up by `|offset|` pixels, revealing
/// world content at position `(-offset_x, -offset_y)`.  For example the start
/// menu uses `(-1808, -864)`, which starts the viewport at world pixel
/// `(1808, 864)`.
///
/// Only map codes that resolve to a DMA entry are rendered; unknown codes are
/// silently skipped.
pub fn render_intro_background(
    jn: &JnFile,
    dma: &DmaFile,
    offset_x: i32,
    offset_y: i32,
) -> Vec<RenderCommand> {
    // World coordinate of the top-left corner of the viewport.
    let world_x = -offset_x;
    let world_y = -offset_y;

    // Tile coordinate of the first (possibly partially visible) tile column/row.
    let start_tile_x = div_floor(world_x, BLOCK_SIZE_I);
    let start_tile_y = div_floor(world_y, BLOCK_SIZE_I);

    // Sub-tile pixel offset: how many pixels of the first tile are hidden.
    let sub_x = world_x.rem_euclid(BLOCK_SIZE_I);
    let sub_y = world_y.rem_euclid(BLOCK_SIZE_I);

    // Number of tile columns/rows needed to fill the game area plus one extra
    // for the partially clipped edge tiles.
    let tiles_x = (GAME_AREA_W as i32) / BLOCK_SIZE_I + 2;
    let tiles_y = (GAME_AREA_H as i32) / BLOCK_SIZE_I + 2;

    let mut commands = Vec::new();
    for row in 0..tiles_y {
        for col in 0..tiles_x {
            let tile_x = start_tile_x + col;
            let tile_y = start_tile_y + row;

            // Reject negative tile coordinates before casting to `usize`.
            if tile_x < 0 || tile_y < 0 {
                continue;
            }

            // map_code returns None for out-of-bounds tile coordinates.
            let Some(map_code) = jn.background().map_code(tile_x as usize, tile_y as usize) else {
                continue;
            };

            let Some(entry) = dma.get_by_map_code(map_code) else {
                continue;
            };

            let game_x = col * BLOCK_SIZE_I - sub_x;
            let game_y = row * BLOCK_SIZE_I - sub_y;
            commands.push(game_area_blit(
                entry.tileset(),
                u16::from(entry.tile()),
                game_x,
                game_y,
                false,
            ));
        }
    }
    commands
}

/// Computes signed floor division, equivalent to `floor(value / divisor)`.
///
/// Standard Rust integer division truncates toward zero; for negative numerators
/// this differs from mathematical floor division by one. This function corrects
/// that so tile coordinates derived from negative world offsets are accurate.
fn div_floor(value: i32, divisor: i32) -> i32 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    // If the remainder has opposite sign from the divisor, we overshot by one.
    if remainder != 0 && (remainder < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::div_floor;

    /// Unit under test: `div_floor` rounding behavior for positive inputs.
    #[test]
    fn div_floor_positive_matches_truncation() {
        assert_eq!(div_floor(17, 16), 1);
        assert_eq!(div_floor(16, 16), 1);
        assert_eq!(div_floor(15, 16), 0);
        assert_eq!(div_floor(0, 16), 0);
    }

    /// Unit under test: `div_floor` floors negative inputs toward negative infinity.
    #[test]
    fn div_floor_negative_floors_toward_negative_infinity() {
        assert_eq!(div_floor(-1, 16), -1);
        assert_eq!(div_floor(-16, 16), -1);
        assert_eq!(div_floor(-17, 16), -2);
    }
}
