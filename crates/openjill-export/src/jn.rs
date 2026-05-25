//! JN export helpers — map background PNG rendering.

use image::{Rgba, RgbaImage};
use openjill_core::entity::Rect;
use openjill_core::Palette;
use openjill_data::dma::{DmaEntry, DmaFile};
use openjill_data::jn::{BACKGROUND_HEIGHT, BACKGROUND_WIDTH, JnFile};
use openjill_data::sha::{ShaFile, ShaTile, ShaTileSet};
use std::collections::HashMap;

/// Width of one JN background cell in pixels.
const CELL_SIZE_PX: i32 = 16;
/// Full rendered map width in pixels (`128 * 16`).
const MAP_WIDTH_PX: i32 = BACKGROUND_WIDTH as i32 * CELL_SIZE_PX;
/// Full rendered map height in pixels (`64 * 16`).
const MAP_HEIGHT_PX: i32 = BACKGROUND_HEIGHT as i32 * CELL_SIZE_PX;
/// Transparent sentinel map code in JN backgrounds.
const TRANSPARENT_MAP_CODE: u16 = 0;

/// Converts one parsed `*.JN1` map into an in-memory RGBA image.
///
/// The result always covers the full JN background area (`128 × 64` cells at
/// `16 × 16` pixels per cell). Map codes resolve through `dma` to
/// `(tileset, tile)` entries, tile pixels are read from `sha`, and indexed
/// pixels are expanded through `palette`.
pub fn map_to_png(jn: &JnFile, sha: &ShaFile, dma: &DmaFile, palette: &Palette) -> RgbaImage {
    map_to_png_with_viewport(jn, sha, dma, palette, None)
}

/// Converts one parsed `*.JN1` map into an in-memory RGBA image, clipped to an
/// optional viewport.
///
/// `viewport` coordinates are in full-map pixel space where `(0, 0)` is the
/// top-left map pixel. When `viewport` is `None`, the full map is rendered.
/// When a viewport is supplied, it is intersected with map bounds and only that
/// sub-region is emitted.
pub fn map_to_png_with_viewport(
    jn: &JnFile,
    sha: &ShaFile,
    dma: &DmaFile,
    palette: &Palette,
    viewport: Option<Rect>,
) -> RgbaImage {
    let map_bounds = Rect::new(0, 0, MAP_WIDTH_PX, MAP_HEIGHT_PX);
    let render_rect = intersect_rect(viewport.unwrap_or(map_bounds), map_bounds);
    if render_rect.w <= 0 || render_rect.h <= 0 {
        return RgbaImage::new(0, 0);
    }

    let mut image = RgbaImage::new(render_rect.w as u32, render_rect.h as u32);
    let tilesets_by_index: HashMap<usize, _> = sha
        .tilesets()
        .iter()
        .map(|tileset| (tileset.entry_index(), tileset))
        .collect();

    let start_cell_x = (render_rect.x / CELL_SIZE_PX) as usize;
    let start_cell_y = (render_rect.y / CELL_SIZE_PX) as usize;
    let end_cell_x = ((render_rect.x + render_rect.w + CELL_SIZE_PX - 1) / CELL_SIZE_PX) as usize;
    let end_cell_y = ((render_rect.y + render_rect.h + CELL_SIZE_PX - 1) / CELL_SIZE_PX) as usize;

    for cell_x in start_cell_x..end_cell_x {
        for cell_y in start_cell_y..end_cell_y {
            let Some(map_code) = jn.background().map_code(cell_x, cell_y) else {
                continue;
            };
            if map_code == TRANSPARENT_MAP_CODE {
                continue;
            }

            let Some(entry) = dma.get_by_map_code(map_code) else {
                continue;
            };
            let Some(tile) = resolve_tile(entry, &tilesets_by_index) else {
                continue;
            };

            blit_tile(
                &mut image,
                tile,
                palette,
                render_rect,
                Rect::new(
                    (cell_x as i32) * CELL_SIZE_PX,
                    (cell_y as i32) * CELL_SIZE_PX,
                    CELL_SIZE_PX,
                    CELL_SIZE_PX,
                ),
            );
        }
    }

    image
}

/// Resolves one DMA entry to a SHA tile, returning `None` when either the
/// tileset or tile index is out of range.
fn resolve_tile<'a>(
    entry: &DmaEntry,
    tilesets_by_index: &HashMap<usize, &'a ShaTileSet>,
) -> Option<&'a ShaTile> {
    let tileset = tilesets_by_index.get(&(entry.tileset() as usize))?;
    tileset.tiles().get(entry.tile() as usize)
}

/// Blits one indexed SHA tile into the destination image, clipped to the
/// current render rectangle and the tile's actual pixel dimensions.
fn blit_tile(
    image: &mut RgbaImage,
    tile: &ShaTile,
    palette: &Palette,
    render_rect: Rect,
    tile_rect: Rect,
) {
    let overlap = intersect_rect(tile_rect, render_rect);
    if overlap.w <= 0 || overlap.h <= 0 {
        return;
    }

    let tile_width = i32::from(tile.width());
    let tile_height = i32::from(tile.height());
    let tile_pixels = tile.indexed_pixels();

    for world_y in overlap.y..(overlap.y + overlap.h) {
        let src_y = world_y - tile_rect.y;
        if src_y < 0 || src_y >= tile_height {
            continue;
        }

        for world_x in overlap.x..(overlap.x + overlap.w) {
            let src_x = world_x - tile_rect.x;
            if src_x < 0 || src_x >= tile_width {
                continue;
            }

            let src_index = (src_y as usize * tile_width as usize) + src_x as usize;
            let palette_index = tile_pixels[src_index];
            if palette_index == 0 {
                continue;
            }
            let [r, g, b, a] = palette.rgba(palette_index);

            image.put_pixel(
                (world_x - render_rect.x) as u32,
                (world_y - render_rect.y) as u32,
                Rgba([r, g, b, a]),
            );
        }
    }
}

/// Computes the intersection of two rectangles using half-open bounds.
fn intersect_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = a.x.saturating_add(a.w).min(b.x.saturating_add(b.w));
    let y1 = a.y.saturating_add(a.h).min(b.y.saturating_add(b.h));
    Rect::new(
        x0,
        y0,
        x1.saturating_sub(x0).max(0),
        y1.saturating_sub(y0).max(0),
    )
}

#[cfg(test)]
mod tests {
    use super::{CELL_SIZE_PX, MAP_HEIGHT_PX, MAP_WIDTH_PX, map_to_png, map_to_png_with_viewport};
    use assert2::check;
    use image::Rgba;
    use openjill_core::entity::Rect;
    use openjill_core::Palette;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;

    /// Unit under test: [`map_to_png`] full-map rendering.
    ///
    /// Preconditions: synthetic JN data with one non-zero cell at `(0, 0)`,
    /// DMA mapping map code `1` to SHA tileset `0` tile `0`, and SHA tileset 0
    /// tile 0 encoded as a `16 × 16` tile filled with palette index `7`.
    ///
    /// Invariants asserted: output dimensions are the full map dimensions and
    /// the rendered tile appears at `(0, 0)` with the expected RGBA color.
    #[test]
    fn renders_full_map_dimensions_and_resolves_tile() {
        let jn = JnFile::from_bytes(jn_bytes_with_cells(&[(0, 0, 1)])).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 0)).expect("DMA should parse");
        let sha = ShaFile::from_bytes(sha_bytes_single_tile(7)).expect("SHA should parse");
        let palette = palette_with_index_color(7, [0x11, 0x22, 0x33]);

        let image = map_to_png(&jn, &sha, &dma, &palette);

        check!(image.width() == MAP_WIDTH_PX as u32);
        check!(image.height() == MAP_HEIGHT_PX as u32);
        check!(*image.get_pixel(0, 0) == Rgba([0x11, 0x22, 0x33, 255]));
        check!(*image.get_pixel(CELL_SIZE_PX as u32, 0) == Rgba([0, 0, 0, 0]));
    }

    /// Unit under test: [`map_to_png_with_viewport`] clipping behavior.
    ///
    /// Preconditions: synthetic JN/DMA/SHA data with one filled tile at world
    /// tile `(1, 1)` and viewport set to a `16 × 16` rectangle at pixel
    /// `(16, 16)` covering exactly that tile.
    ///
    /// Invariants asserted: result dimensions equal the viewport dimensions and
    /// every pixel in the viewport is rendered from the selected tile.
    #[test]
    fn renders_viewport_sub_region() {
        let jn = JnFile::from_bytes(jn_bytes_with_cells(&[(1, 1, 2)])).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(2, 0, 0)).expect("DMA should parse");
        let sha = ShaFile::from_bytes(sha_bytes_single_tile(9)).expect("SHA should parse");
        let palette = palette_with_index_color(9, [0xaa, 0xbb, 0xcc]);

        let image = map_to_png_with_viewport(
            &jn,
            &sha,
            &dma,
            &palette,
            Some(Rect::new(16, 16, 16, 16)),
        );

        check!(image.width() == 16);
        check!(image.height() == 16);
        check!(*image.get_pixel(0, 0) == Rgba([0xaa, 0xbb, 0xcc, 255]));
        check!(*image.get_pixel(15, 15) == Rgba([0xaa, 0xbb, 0xcc, 255]));
    }

    /// Unit under test: transparent palette index handling.
    ///
    /// Preconditions: synthetic JN/DMA/SHA data with a visible map tile whose
    /// indexed pixels are all zero.
    ///
    /// Invariants asserted: output remains transparent for those tile pixels.
    #[test]
    fn index_zero_pixels_remain_transparent() {
        let jn = JnFile::from_bytes(jn_bytes_with_cells(&[(0, 0, 1)])).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 0)).expect("DMA should parse");
        let sha = ShaFile::from_bytes(sha_bytes_single_tile(0)).expect("SHA should parse");
        let palette = palette_with_index_color(0, [0xff, 0x00, 0x00]);

        let image = map_to_png(&jn, &sha, &dma, &palette);

        check!(*image.get_pixel(0, 0) == Rgba([0, 0, 0, 0]));
    }

    /// Unit under test: [`intersect_rect`] endpoint overflow resilience.
    ///
    /// Preconditions: one rectangle with endpoint past `i32::MAX`.
    ///
    /// Invariants asserted: saturation preserves a valid clipped result.
    #[test]
    fn intersect_rect_saturates_endpoint_overflow() {
        let overlap = super::intersect_rect(Rect::new(i32::MAX - 1, 0, 10, 10), Rect::new(0, 0, 10, 10));

        check!(overlap == Rect::new(i32::MAX - 1, 0, 0, 10));
    }

    /// Builds synthetic JN bytes with selected non-zero background cells.
    fn jn_bytes_with_cells(cells: &[(usize, usize, u16)]) -> Vec<u8> {
        const BACKGROUND_BYTES: usize = 128 * 64 * 2;
        const BACKGROUND_HEIGHT: usize = 64;
        const SAVE_BLOCK_BYTES: usize = 70;
        let mut bytes = vec![0u8; BACKGROUND_BYTES + 2 + SAVE_BLOCK_BYTES];

        for (x, y, map_code) in cells {
            let cell_index = x * BACKGROUND_HEIGHT + y;
            let offset = cell_index * 2;
            bytes[offset..offset + 2].copy_from_slice(&map_code.to_le_bytes());
        }

        bytes
    }

    /// Builds one synthetic DMA entry.
    fn dma_bytes_single(map_code: u16, tile: u8, tileset: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&map_code.to_le_bytes());
        bytes.push(tile);
        bytes.push(tileset);
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.push(0);
        bytes
    }

    /// Builds a synthetic SHA file with header entry 0 containing one
    /// `16 × 16` tile in tileset 0.
    fn sha_bytes_single_tile(fill_index: u8) -> Vec<u8> {
        const HEADER_SIZE: usize = 128 * 4 + 128 * 2;
        let mut bytes = vec![0u8; HEADER_SIZE];

        let tileset_offset = HEADER_SIZE as u32;
        let tile_pixels = vec![fill_index; 16 * 16];
        let mut tileset_bytes = Vec::new();
        tileset_bytes.push(1); // tile_count
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes()); // rotations
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes()); // cga_size
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes()); // ega_size
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes()); // vga_size
        tileset_bytes.push(8); // bit_depth
        tileset_bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        tileset_bytes.push(16); // width
        tileset_bytes.push(16); // height
        tileset_bytes.push(0); // data_format
        tileset_bytes.extend(tile_pixels);

        let tileset_size = u16::try_from(tileset_bytes.len()).expect("synthetic tileset fits in u16");
        bytes[0..4].copy_from_slice(&tileset_offset.to_le_bytes());
        let size_offset = 128 * 4;
        bytes[size_offset..size_offset + 2].copy_from_slice(&tileset_size.to_le_bytes());
        bytes.extend(tileset_bytes);
        bytes
    }

    /// Builds a palette with one explicitly configured color index.
    fn palette_with_index_color(index: u8, rgb: [u8; 3]) -> Palette {
        let mut entries = [[0u8; 3]; 256];
        entries[index as usize] = rgb;
        Palette::new(entries)
    }
}
