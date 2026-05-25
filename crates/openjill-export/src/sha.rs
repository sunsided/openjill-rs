//! SHA export helpers — tileset and atlas PNG rendering.
//!
//! This module converts parsed `*.SHA` data into RGBA images that can be
//! saved as PNG with the [`image`] crate. Pixel values in the parsed data
//! are VGA palette indices (0–255); the [`TilesetColorOutput`] controls
//! whether those indices are kept as-is or resolved to full RGB colour.
//!
//! # Screen-mode filters
//!
//! The original DOS engine supported three video modes: CGA (4-colour), EGA
//! (16-colour), and VGA (256-colour). The SHA parser stores pixel data as VGA
//! palette indices regardless of the original source mode, so the
//! [`ScreenMode`] filter here controls *which tilesets* are included in an
//! atlas export rather than applying a different colour pipeline:
//!
//! - [`ScreenMode::Vga`] — include every tileset (8-bit and below).
//! - [`ScreenMode::Ega`] / [`ScreenMode::Cga`] — include only tilesets with
//!   `bit_depth < 8`; 8-bit VGA-exclusive tilesets are omitted because they
//!   could not be rendered in those lower-colour modes.

use image::{Rgba, RgbaImage};
use openjill_data::sha::{ShaFile, ShaTileSet};
use std::sync::Arc;

/// Selects how indexed SHA tiles are turned into export pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TilesetColorOutput {
    /// Preserve palette indices directly in RGBA channels.
    ///
    /// Each pixel index `i` is written as `R=i, G=i, B=i, A=255`.
    /// This keeps indexed semantics intact for tools that post-process indices.
    Indexed,
    /// Expand indices to RGB values using an explicit VGA palette.
    ///
    /// Each index `i` is resolved through `palette[i]` and emitted as
    /// `R, G, B` from that triplet with `A=255`.
    Colored {
        /// 256-entry VGA palette used to resolve tile indices to RGB.
        palette: Arc<[[u8; 3]; 256]>,
    },
}

/// Screen mode used to filter which tilesets are included in an atlas export.
///
/// Pixel rendering always uses the VGA-expanded indices stored by the SHA
/// parser; this enum only controls which tilesets pass the bit-depth filter.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScreenMode {
    /// CGA (4-colour) screen mode.
    ///
    /// Only tilesets with `bit_depth < 8` are included; 8-bit VGA-exclusive
    /// tilesets are skipped.
    Cga,
    /// EGA (16-colour) screen mode.
    ///
    /// Only tilesets with `bit_depth < 8` are included; 8-bit VGA-exclusive
    /// tilesets are skipped.
    Ega,
    /// VGA (256-colour) screen mode.
    ///
    /// All tilesets are included regardless of bit depth.
    #[default]
    Vga,
}

/// Filter controlling which tile kinds appear in an atlas export.
///
/// Both `fonts` and `pictures` default to `true` so that the default
/// [`AtlasOptions`] exports the complete set of tilesets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TileFilter {
    /// When `true`, font tilesets (whose `SHM_FONTF` flag is set) are included.
    pub fonts: bool,
    /// When `true`, non-font (picture / sprite) tilesets are included.
    pub pictures: bool,
}

impl Default for TileFilter {
    /// Returns a filter that includes both font and picture tilesets.
    fn default() -> Self {
        Self {
            fonts: true,
            pictures: true,
        }
    }
}

/// Options that control the [`atlas_to_png`] export.
#[derive(Clone, Debug)]
pub struct AtlasOptions {
    /// Controls whether pixels are kept as palette indices or resolved to RGB.
    pub output: TilesetColorOutput,
    /// Screen-mode filter that determines which tilesets are included.
    pub mode: ScreenMode,
    /// Tile-kind filter that selects fonts, pictures, or both.
    pub filter: TileFilter,
    /// Pixel gap inserted between tiles in the grid layout.
    ///
    /// A value of `0` (the default) produces a packed grid with no space
    /// between tiles.
    pub padding: usize,
}

impl Default for AtlasOptions {
    /// Returns options that export all tilesets in VGA mode with no padding
    /// and indexed pixel output.
    fn default() -> Self {
        Self {
            output: TilesetColorOutput::Indexed,
            mode: ScreenMode::Vga,
            filter: TileFilter::default(),
            padding: 0,
        }
    }
}

/// Converts one parsed `*.SHA` tileset into an in-memory RGBA image.
///
/// All tiles in the tileset are laid out in a square-ish row-major grid with
/// no padding between tiles. The `output` parameter controls how each indexed
/// pixel is turned into an RGBA value:
///
/// - [`TilesetColorOutput::Indexed`] — writes `R=i, G=i, B=i, A=255`.
/// - [`TilesetColorOutput::Colored`] — resolves `i` through the supplied
///   palette and writes `R, G, B, A=255`.
///
/// Returns a 1×1 transparent black image when the tileset contains no tiles
/// (or only zero-area tiles).
///
/// Callers can encode the returned [`RgbaImage`] as PNG via the `image` crate
/// without any additional PNG dependency.
pub fn tileset_to_png(tileset: &ShaTileSet, output: TilesetColorOutput) -> RgbaImage {
    let tiles: Vec<_> = tileset
        .tiles()
        .iter()
        .filter_map(|tile| {
            let w = usize::from(tile.width());
            let h = usize::from(tile.height());
            (w > 0 && h > 0).then_some((tile, w, h))
        })
        .collect();
    if tiles.is_empty() {
        return RgbaImage::new(1, 1);
    }

    let cols = grid_columns(tiles.len());

    // First pass: compute per-tile placement coordinates.
    let mut placements: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(tiles.len());
    let mut x = 0usize;
    let mut y = 0usize;
    let mut row_height = 0usize;
    let mut atlas_width = 0usize;
    let mut col_idx = 0usize;

    for (_, w, h) in &tiles {
        if col_idx == cols {
            atlas_width = atlas_width.max(x);
            y += row_height;
            x = 0;
            row_height = 0;
            col_idx = 0;
        }
        placements.push((x, y, *w, *h));
        row_height = row_height.max(*h);
        x += *w;
        col_idx += 1;
    }
    atlas_width = atlas_width.max(x);
    let atlas_height = y + row_height;

    // Second pass: blit pixels into the RGBA image.
    let mut image = RgbaImage::new(
        atlas_width as u32,
        atlas_height as u32,
    );

    for ((px, py, pw, ph), (tile, ..)) in placements.iter().zip(&tiles) {
        let pixels = tile.indexed_pixels();
        for row in 0..*ph {
            for col in 0..*pw {
                let index = pixels[row * pw + col];
                let rgba = expand_pixel(index, &output);
                image.put_pixel((px + col) as u32, (py + row) as u32, rgba);
            }
        }
    }

    image
}

/// Converts all matching tilesets in a parsed `*.SHA` file into one RGBA
/// atlas image.
///
/// Tilesets are selected by [`AtlasOptions::mode`] (bit-depth filter) and
/// [`AtlasOptions::filter`] (font vs. picture filter). The surviving tiles
/// are arranged in a square-ish row-major grid separated by
/// [`AtlasOptions::padding`] pixels. Pixel values are expanded according to
/// [`AtlasOptions::output`].
///
/// Returns a 1×1 transparent black image when no tiles pass the filter
/// (including when all matching tiles are zero-area).
///
/// Callers can encode the returned [`RgbaImage`] as PNG via the `image` crate
/// without any additional PNG dependency.
pub fn atlas_to_png(sha: &ShaFile, options: &AtlasOptions) -> RgbaImage {
    // Collect tiles from tilesets that survive both filters.
    // Tuple layout: (width, height, indexed_pixels).
    let tiles: Vec<(usize, usize, &[u8])> = sha
        .tilesets()
        .iter()
        .filter(|ts| tileset_matches_mode(ts, options.mode))
        .filter(|ts| tileset_matches_filter(ts, options.filter))
        .flat_map(|ts| {
            ts.tiles().iter().filter_map(|tile| {
                let w = usize::from(tile.width());
                let h = usize::from(tile.height());
                (w > 0 && h > 0).then_some((w, h, tile.indexed_pixels()))
            })
        })
        .collect();

    if tiles.is_empty() {
        return RgbaImage::new(1, 1);
    }

    let cols = grid_columns(tiles.len());
    let padding = options.padding;

    // First pass: compute per-tile placement coordinates.
    let mut placements: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(tiles.len());
    let mut x = 0usize;
    let mut y = 0usize;
    let mut row_height = 0usize;
    let mut atlas_width = 0usize;
    let mut col_idx = 0usize;

    for (w, h, ..) in &tiles {
        if col_idx == cols {
            atlas_width = atlas_width.max(x.saturating_sub(padding));
            y += row_height + padding;
            x = 0;
            row_height = 0;
            col_idx = 0;
        }
        placements.push((x, y, *w, *h));
        row_height = row_height.max(*h);
        x += w + padding;
        col_idx += 1;
    }
    atlas_width = atlas_width.max(x.saturating_sub(padding));
    let atlas_height = y + row_height;

    // Second pass: blit pixels.
    let mut image = RgbaImage::new(atlas_width as u32, atlas_height as u32);

    for ((px, py, pw, ph), (_, _, pixels)) in placements.iter().zip(&tiles) {
        for row in 0..*ph {
            for col in 0..*pw {
                let index = pixels[row * pw + col];
                let rgba = expand_pixel(index, &options.output);
                image.put_pixel((px + col) as u32, (py + row) as u32, rgba);
            }
        }
    }

    image
}

/// Returns `true` when `tileset` should be included for the given screen mode.
///
/// VGA mode accepts every tileset; CGA and EGA modes skip 8-bit tilesets
/// because those tilesets were VGA-exclusive in the original engine.
fn tileset_matches_mode(tileset: &ShaTileSet, mode: ScreenMode) -> bool {
    match mode {
        ScreenMode::Vga => true,
        ScreenMode::Cga | ScreenMode::Ega => tileset.bit_depth() < 8,
    }
}

/// Returns `true` when `tileset` should be included given the tile-kind filter.
///
/// Font tilesets (those with `SHM_FONTF` set) are gated by `filter.fonts`;
/// all other tilesets are gated by `filter.pictures`.
fn tileset_matches_filter(tileset: &ShaTileSet, filter: TileFilter) -> bool {
    if tileset.is_font() {
        filter.fonts
    } else {
        filter.pictures
    }
}

/// Expands one palette index to an RGBA pixel according to `output`.
///
/// For [`TilesetColorOutput::Indexed`] the index value fills all three colour
/// channels so the result is a deterministic greyscale encoding of the index.
/// For [`TilesetColorOutput::Colored`] the index is resolved through the
/// supplied VGA palette.
fn expand_pixel(index: u8, output: &TilesetColorOutput) -> Rgba<u8> {
    match output {
        TilesetColorOutput::Indexed => Rgba([index, index, index, 255]),
        TilesetColorOutput::Colored { palette } => {
            let [r, g, b] = palette[usize::from(index)];
            Rgba([r, g, b, 255])
        }
    }
}

/// Returns the smallest `n ≥ 1` such that `n × n ≥ tile_count`.
///
/// Used to determine the column count for square-ish grid layouts.
fn grid_columns(tile_count: usize) -> usize {
    let mut columns = 1usize;
    while columns.saturating_mul(columns) < tile_count {
        columns += 1;
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_data::sha::ShaFile;

    // ---------------------------------------------------------------------------
    // Shared synthetic-fixture builder
    // ---------------------------------------------------------------------------

    /// Builds minimal valid SHA bytes containing:
    ///
    /// - One 2-bit tileset (entry index 0) with a 4-entry colour map and two
    ///   2×2 tiles (4 pixels each). Pixels are raw colour-map indices 0–3.
    /// - One 8-bit tileset (entry index 1) flagged as a font with one 3×3 tile.
    ///   No colour map (8-bit + font).
    ///
    /// The colour-map channels for the 2-bit tileset are:
    ///   entry 0 → cga=1, ega=2, vga=3
    ///   entry 1 → cga=5, ega=6, vga=7
    ///   entry 2 → cga=9, ega=10, vga=11
    ///   entry 3 → cga=13, ega=14, vga=15
    ///
    /// At parse time the Rust parser expands raw 0–3 → vga channel (3, 7, 11, 15)
    /// so `ShaTile::indexed_pixels()` contains those VGA indices.
    fn synthetic_sha_bytes() -> Vec<u8> {
        const ENTRY_COUNT: usize = 128;
        const HEADER_LEN: usize = ENTRY_COUNT * 4 + ENTRY_COUNT * 2;

        // Build tileset 0: 2-bit, picture, 2 tiles of 2×2.
        let ts0: Vec<u8> = {
            let mut b = Vec::new();
            b.push(2u8); // tile_count
            b.extend(0u16.to_le_bytes()); // rotations
            b.extend(4u16.to_le_bytes()); // cga_size (arbitrary)
            b.extend(4u16.to_le_bytes()); // ega_size
            b.extend(4u16.to_le_bytes()); // vga_size
            b.push(2u8); // bit_depth = 2  → color map present
            b.extend(0x0000u16.to_le_bytes()); // flags = 0 (picture, not font)
            // Color map: 2^2 = 4 entries × 4 bytes each
            b.extend([1u8, 2u8, 3u8, 0u8]); // entry 0: cga=1, ega=2, vga=3
            b.extend([5u8, 6u8, 7u8, 0u8]); // entry 1: cga=5, ega=6, vga=7
            b.extend([9u8, 10u8, 11u8, 0u8]); // entry 2: cga=9, ega=10, vga=11
            b.extend([13u8, 14u8, 15u8, 0u8]); // entry 3: cga=13, ega=14, vga=15
            // Tile 0: 2×2, data_format=0, pixels: 0,1,2,3
            b.push(2u8); // width
            b.push(2u8); // height
            b.push(0u8); // data_format
            b.extend([0u8, 1u8, 2u8, 3u8]);
            // Tile 1: 2×2, data_format=0, pixels: 3,2,1,0
            b.push(2u8);
            b.push(2u8);
            b.push(0u8);
            b.extend([3u8, 2u8, 1u8, 0u8]);
            b
        };

        // Build tileset 1: 8-bit font, 1 tile of 3×3.
        let ts1: Vec<u8> = {
            let mut b = Vec::new();
            b.push(1u8); // tile_count
            b.extend(0u16.to_le_bytes()); // rotations
            b.extend(0u16.to_le_bytes()); // cga_size
            b.extend(0u16.to_le_bytes()); // ega_size
            b.extend(0u16.to_le_bytes()); // vga_size
            b.push(8u8); // bit_depth = 8  → no color map
            b.extend(0x0001u16.to_le_bytes()); // flags = SHM_FONTF
            // No colour map for 8-bit.
            // Tile 0: 3×3, pixels 10–18
            b.push(3u8);
            b.push(3u8);
            b.push(0u8);
            b.extend([10u8, 11u8, 12u8, 13u8, 14u8, 15u8, 16u8, 17u8, 18u8]);
            b
        };

        let offset0 = HEADER_LEN;
        let offset1 = offset0 + ts0.len();

        let mut bytes = vec![0u8; HEADER_LEN];
        bytes.extend_from_slice(&ts0);
        bytes.extend_from_slice(&ts1);

        // Write offsets for entries 0 and 1.
        let o0 = (offset0 as u32).to_le_bytes();
        let o1 = (offset1 as u32).to_le_bytes();
        bytes[0..4].copy_from_slice(&o0);
        bytes[4..8].copy_from_slice(&o1);

        // Write sizes.
        let size0 = (ts0.len() as u16).to_le_bytes();
        let size1 = (ts1.len() as u16).to_le_bytes();
        let sizes_base = ENTRY_COUNT * 4;
        bytes[sizes_base..sizes_base + 2].copy_from_slice(&size0);
        bytes[sizes_base + 2..sizes_base + 4].copy_from_slice(&size1);

        bytes
    }

    // ---------------------------------------------------------------------------
    // grid_columns
    // ---------------------------------------------------------------------------

    /// Unit under test: [`grid_columns`].
    ///
    /// Preconditions: various tile counts.
    ///
    /// Invariants asserted: result satisfies `cols * cols >= n` and is the
    /// smallest such integer.
    #[test]
    fn grid_columns_returns_ceiling_sqrt() {
        assert_eq!(grid_columns(1), 1);
        assert_eq!(grid_columns(4), 2);
        assert_eq!(grid_columns(5), 3);
        assert_eq!(grid_columns(9), 3);
        assert_eq!(grid_columns(10), 4);
    }

    // ---------------------------------------------------------------------------
    // expand_pixel
    // ---------------------------------------------------------------------------

    /// Unit under test: [`expand_pixel`] with [`TilesetColorOutput::Indexed`].
    ///
    /// Invariants asserted: all three RGB channels equal the input index and
    /// alpha is always 255.
    #[test]
    fn expand_pixel_indexed_maps_to_greyscale() {
        let out = TilesetColorOutput::Indexed;
        let Rgba([r, g, b, a]) = expand_pixel(42, &out);
        assert_eq!((r, g, b, a), (42, 42, 42, 255));
    }

    /// Unit under test: [`expand_pixel`] with [`TilesetColorOutput::Colored`].
    ///
    /// Preconditions: a synthetic 256-entry palette with index 7 set to
    /// `[100, 150, 200]`.
    ///
    /// Invariants asserted: the emitted pixel matches the palette entry and
    /// alpha is always 255.
    #[test]
    fn expand_pixel_colored_uses_palette() {
        let mut palette = [[0u8; 3]; 256];
        palette[7] = [100, 150, 200];
        let out = TilesetColorOutput::Colored {
            palette: Arc::new(palette),
        };
        let Rgba([r, g, b, a]) = expand_pixel(7, &out);
        assert_eq!((r, g, b, a), (100, 150, 200, 255));
    }

    // ---------------------------------------------------------------------------
    // tileset_matches_mode
    // ---------------------------------------------------------------------------

    /// Unit under test: [`tileset_matches_mode`].
    ///
    /// Preconditions: parsed SHA file with one 2-bit picture tileset (index 0)
    /// and one 8-bit font tileset (index 1).
    ///
    /// Invariants asserted:
    /// - VGA mode includes both tilesets.
    /// - CGA and EGA modes include only the 2-bit tileset.
    #[test]
    fn screen_mode_filter_excludes_8bit_for_cga_ega() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let tilesets = sha.tilesets();
        let pic = &tilesets[0]; // 2-bit picture
        let fnt = &tilesets[1]; // 8-bit font

        assert!(tileset_matches_mode(pic, ScreenMode::Vga));
        assert!(tileset_matches_mode(fnt, ScreenMode::Vga));

        assert!(tileset_matches_mode(pic, ScreenMode::Cga));
        assert!(!tileset_matches_mode(fnt, ScreenMode::Cga));

        assert!(tileset_matches_mode(pic, ScreenMode::Ega));
        assert!(!tileset_matches_mode(fnt, ScreenMode::Ega));
    }

    // ---------------------------------------------------------------------------
    // tileset_matches_filter
    // ---------------------------------------------------------------------------

    /// Unit under test: [`tileset_matches_filter`].
    ///
    /// Preconditions: parsed SHA file with one picture and one font tileset.
    ///
    /// Invariants asserted: the filter correctly gates font-only and
    /// picture-only requests.
    #[test]
    fn tile_filter_gates_fonts_and_pictures_independently() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let tilesets = sha.tilesets();
        let pic = &tilesets[0];
        let fnt = &tilesets[1];

        let all = TileFilter { fonts: true, pictures: true };
        assert!(tileset_matches_filter(pic, all));
        assert!(tileset_matches_filter(fnt, all));

        let fonts_only = TileFilter { fonts: true, pictures: false };
        assert!(!tileset_matches_filter(pic, fonts_only));
        assert!(tileset_matches_filter(fnt, fonts_only));

        let pictures_only = TileFilter { fonts: false, pictures: true };
        assert!(tileset_matches_filter(pic, pictures_only));
        assert!(!tileset_matches_filter(fnt, pictures_only));

        let none = TileFilter { fonts: false, pictures: false };
        assert!(!tileset_matches_filter(pic, none));
        assert!(!tileset_matches_filter(fnt, none));
    }

    // ---------------------------------------------------------------------------
    // tileset_to_png
    // ---------------------------------------------------------------------------

    /// Unit under test: [`tileset_to_png`] with [`TilesetColorOutput::Indexed`].
    ///
    /// Preconditions: the 2-bit picture tileset from the synthetic fixture has
    /// two 2×2 tiles. The parser expands raw indices 0–3 through the colour
    /// map's `vga` channel (3, 7, 11, 15).
    ///
    /// Invariants asserted: image dimensions match the expected grid layout,
    /// and the first four pixels of the first tile carry the VGA-expanded
    /// values in greyscale RGBA.
    #[test]
    fn tileset_to_png_indexed_preserves_vga_expanded_values() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let tileset = &sha.tilesets()[0]; // 2-bit picture tileset

        let image = tileset_to_png(tileset, TilesetColorOutput::Indexed);

        // Two 2×2 tiles → 2-column grid → 4×2 image.
        assert_eq!(image.width(), 4, "atlas width: 2 columns × 2px each");
        assert_eq!(image.height(), 2, "atlas height: 1 row × 2px");

        // Tile 0 pixels (raw 0,1,2,3 → vga 3,7,11,15) at top-left 2×2 block.
        let expected_indices = [3u8, 7, 11, 15];
        for (idx, &exp) in expected_indices.iter().enumerate() {
            let x = (idx % 2) as u32;
            let y = (idx / 2) as u32;
            let Rgba([r, g, b, a]) = *image.get_pixel(x, y);
            assert_eq!((r, g, b, a), (exp, exp, exp, 255), "pixel ({x},{y})");
        }

        // Tile 1 pixels (raw 3,2,1,0 → vga 15,11,7,3) at top-right 2×2 block.
        let expected_tile1 = [15u8, 11, 7, 3];
        for (idx, &exp) in expected_tile1.iter().enumerate() {
            let x = 2 + (idx % 2) as u32;
            let y = (idx / 2) as u32;
            let Rgba([r, g, b, a]) = *image.get_pixel(x, y);
            assert_eq!((r, g, b, a), (exp, exp, exp, 255), "pixel ({x},{y})");
        }
    }

    /// Unit under test: [`tileset_to_png`] with [`TilesetColorOutput::Colored`].
    ///
    /// Preconditions: a synthetic palette is built where each index maps to
    /// `[index, index, index]` so the output is identical to indexed mode for
    /// low indices but exercised through the palette path.
    ///
    /// Invariants asserted: the first pixel of tile 0 has `R=3, G=3, B=3`.
    #[test]
    fn tileset_to_png_colored_expands_through_palette() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let tileset = &sha.tilesets()[0];

        // Identity palette: index i → [i, i, i]
        let mut pal = [[0u8; 3]; 256];
        for (i, entry) in pal.iter_mut().enumerate() {
            *entry = [i as u8, i as u8, i as u8];
        }
        let out = TilesetColorOutput::Colored {
            palette: Arc::new(pal),
        };

        let image = tileset_to_png(tileset, out);
        // Tile 0, pixel 0 → vga index 3 → palette[3] = [3,3,3]
        let Rgba([r, g, b, a]) = *image.get_pixel(0, 0);
        assert_eq!((r, g, b, a), (3, 3, 3, 255));
    }

    /// Unit under test: [`tileset_to_png`] on a tileset with no tiles.
    ///
    /// Invariants asserted: returns a 1×1 image rather than panicking.
    #[test]
    fn tileset_to_png_empty_returns_one_by_one() {
        // Build a minimal SHA with one tileset that has tile_count = 0.
        const ENTRY_COUNT: usize = 128;
        const HEADER_LEN: usize = ENTRY_COUNT * 4 + ENTRY_COUNT * 2;
        let ts: Vec<u8> = {
            let mut b = Vec::new();
            b.push(0u8); // tile_count = 0
            b.extend(0u16.to_le_bytes()); // rotations
            b.extend(0u16.to_le_bytes()); // cga_size
            b.extend(0u16.to_le_bytes()); // ega_size
            b.extend(0u16.to_le_bytes()); // vga_size
            b.push(8u8); // bit_depth
            b.extend(0x0001u16.to_le_bytes()); // flags = font
            b
        };
        let offset = HEADER_LEN as u32;
        let size = ts.len() as u16;
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes.extend_from_slice(&ts);
        bytes[0..4].copy_from_slice(&offset.to_le_bytes());
        bytes[ENTRY_COUNT * 4..ENTRY_COUNT * 4 + 2].copy_from_slice(&size.to_le_bytes());

        let sha = ShaFile::from_bytes(bytes).unwrap();
        let tileset = &sha.tilesets()[0];

        let image = tileset_to_png(tileset, TilesetColorOutput::Indexed);
        assert_eq!((image.width(), image.height()), (1, 1));
    }

    // ---------------------------------------------------------------------------
    // atlas_to_png
    // ---------------------------------------------------------------------------

    /// Unit under test: [`atlas_to_png`] with default [`AtlasOptions`].
    ///
    /// Preconditions: synthetic SHA with one 2-bit picture tileset (2 tiles of
    /// 2×2) and one 8-bit font tileset (1 tile of 3×3). Default options use VGA
    /// mode, all tile types, indexed output, and zero padding.
    ///
    /// Invariants asserted: the atlas includes all three tiles and the first
    /// pixels of each tile carry the correct VGA-expanded index values.
    #[test]
    fn atlas_to_png_default_includes_all_tiles() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions::default();

        let image = atlas_to_png(&sha, &opts);

        // 3 tiles total → 2-column grid
        // Row 0: tile0 (2×2) + tile1 (2×2) → x: 0..4, y: 0..2
        // Row 1: tile2 (3×3)               → x: 0..3, y: 2..5
        assert!(image.width() >= 4, "image wide enough for row 0");
        assert!(image.height() >= 5, "image tall enough for 2 rows");

        // Tile 0, pixel (0,0) → vga index 3 → greyscale 3.
        let Rgba([r, g, b, a]) = *image.get_pixel(0, 0);
        assert_eq!((r, g, b, a), (3, 3, 3, 255));
    }

    /// Unit under test: [`atlas_to_png`] with [`ScreenMode::Cga`].
    ///
    /// Preconditions: synthetic SHA with one 2-bit picture tileset and one
    /// 8-bit font tileset.
    ///
    /// Invariants asserted: only the 2-bit tileset tiles appear (2 tiles of
    /// 2×2); the 8-bit font tileset is excluded.
    #[test]
    fn atlas_to_png_cga_mode_excludes_8bit_tilesets() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions {
            mode: ScreenMode::Cga,
            filter: TileFilter {
                fonts: true,
                pictures: true,
            },
            ..AtlasOptions::default()
        };

        let image = atlas_to_png(&sha, &opts);

        // Only 2 tiles of 2×2 each → 2-column grid → 4×2 image.
        assert_eq!((image.width(), image.height()), (4, 2));
    }

    /// Unit under test: [`atlas_to_png`] with font-only [`TileFilter`].
    ///
    /// Preconditions: synthetic SHA with one picture tileset (2 tiles) and
    /// one font tileset (1 tile of 3×3). The filter includes only fonts.
    ///
    /// Invariants asserted: the atlas contains only the single font tile;
    /// dimensions are 3×3.
    #[test]
    fn atlas_to_png_font_only_filter_excludes_pictures() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions {
            filter: TileFilter {
                fonts: true,
                pictures: false,
            },
            ..AtlasOptions::default()
        };

        let image = atlas_to_png(&sha, &opts);

        // Only the 3×3 font tile.
        assert_eq!((image.width(), image.height()), (3, 3));
    }

    /// Unit under test: [`atlas_to_png`] with picture-only [`TileFilter`].
    ///
    /// Preconditions: synthetic SHA with two 2×2 picture tiles and one 3×3
    /// font tile. The filter includes only pictures.
    ///
    /// Invariants asserted: the atlas contains only the two picture tiles;
    /// dimensions match a 2-column grid of 2×2 tiles (4×2).
    #[test]
    fn atlas_to_png_picture_only_filter_excludes_fonts() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions {
            filter: TileFilter {
                fonts: false,
                pictures: true,
            },
            ..AtlasOptions::default()
        };

        let image = atlas_to_png(&sha, &opts);
        assert_eq!((image.width(), image.height()), (4, 2));
    }

    /// Unit under test: [`atlas_to_png`] with non-zero padding.
    ///
    /// Preconditions: synthetic SHA with two 2×2 picture tiles. Padding of 1
    /// is applied.
    ///
    /// Invariants asserted: the atlas width is 5 (2 + 1 + 2) and height is 2.
    #[test]
    fn atlas_to_png_padding_inserts_gap_between_tiles() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions {
            filter: TileFilter {
                fonts: false,
                pictures: true,
            },
            padding: 1,
            ..AtlasOptions::default()
        };

        let image = atlas_to_png(&sha, &opts);
        // 2 tiles of width 2, padding 1 → total width = 2 + 1 + 2 = 5, height = 2
        assert_eq!((image.width(), image.height()), (5, 2));
    }

    /// Unit under test: [`atlas_to_png`] when no tilesets pass the filter.
    ///
    /// Invariants asserted: a 1×1 image is returned without panicking.
    #[test]
    fn atlas_to_png_empty_filter_returns_one_by_one() {
        let sha = ShaFile::from_bytes(synthetic_sha_bytes()).unwrap();
        let opts = AtlasOptions {
            filter: TileFilter {
                fonts: false,
                pictures: false,
            },
            ..AtlasOptions::default()
        };

        let image = atlas_to_png(&sha, &opts);
        assert_eq!((image.width(), image.height()), (1, 1));
    }
}
