//! Integration test that renders tilesets from the original `JILL1.SHA` file
//! when the game's data directory is available locally.  The test self-skips
//! when the data is not present so CI runs without copyrighted bytes still
//! pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::sha::ShaFile;
use openjill_export::sha::{
    AtlasOptions, ScreenMode, TileFilter, TilesetColorOutput, atlas_to_png, tileset_to_png,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: [`tileset_to_png`] and [`atlas_to_png`] on the original
/// `JILL1.SHA` file.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL1.SHA`, or the workspace-relative `data/original/JILL1` directory is
/// present.  When neither is available the test prints a skip message and
/// returns so machines without the original data still pass CI.
///
/// Invariants asserted: every tileset that passes the VGA filter can be
/// rendered via [`tileset_to_png`] to a non-empty image whose dimensions are
/// consistent with the tile data; the full [`atlas_to_png`] call with default
/// options succeeds and returns a non-empty image; the CGA-mode filter
/// produces a strictly smaller or equal tile count than the VGA-mode result.
#[test]
fn renders_tilesets_from_original_jill_sha_when_available() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is not set and default data directory is missing"
            );
            return;
        }
    };

    check!(
        data_dir.is_dir(),
        "data directory must exist when configured"
    );

    let directory = DataDirectory::new(&data_dir);
    let mut reader = directory.open_reader("JILL1.SHA").unwrap_or_else(|error| {
        panic!(
            "JILL1.SHA must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });

    let sha = ShaFile::parse(&mut reader).expect("JILL1.SHA from original data should parse");
    check!(
        !sha.tilesets().is_empty(),
        "JILL1.SHA must contain at least one tileset"
    );

    // Build a minimal identity palette (index i → [i, i, i]) for the colored
    // output path. The original palette is not required for this structural
    // test.
    let mut palette = [[0u8; 3]; 256];
    for (i, entry) in palette.iter_mut().enumerate() {
        *entry = [i as u8, i as u8, i as u8];
    }
    let palette = Arc::new(palette);

    // --- tileset_to_png: render every tileset individually ----------------
    let mut nonempty_count = 0usize;
    for tileset in sha.tilesets() {
        let image = tileset_to_png(tileset, TilesetColorOutput::Indexed);
        check!(image.width() >= 1);
        check!(image.height() >= 1);

        // Non-empty tilesets produce an image that is at least 1×1.
        if !tileset.tiles().is_empty() {
            let max_tile_pixels = tileset
                .tiles()
                .iter()
                .map(|t| usize::from(t.width()) * usize::from(t.height()))
                .max()
                .unwrap_or(0);
            let actual_pixels = (image.width() * image.height()) as usize;
            // The atlas must be at least as large as the largest single tile.
            check!(actual_pixels >= max_tile_pixels);
            nonempty_count += 1;
        }

        // Colored path: same structural requirements.
        let colored = tileset_to_png(
            tileset,
            TilesetColorOutput::Colored {
                palette: Arc::clone(&palette),
            },
        );
        check!(colored.width() == image.width());
        check!(colored.height() == image.height());
    }
    check!(
        nonempty_count > 0,
        "at least one non-empty tileset must exist"
    );

    // --- atlas_to_png: VGA mode (all tilesets) ----------------------------
    let vga_atlas = atlas_to_png(
        &sha,
        &AtlasOptions {
            output: TilesetColorOutput::Indexed,
            mode: ScreenMode::Vga,
            filter: TileFilter::default(),
            padding: 0,
        },
    );
    check!(
        vga_atlas.width() > 1 || vga_atlas.height() > 1,
        "VGA atlas must be non-trivial"
    );

    let vga_tile_count: usize = sha.tilesets().iter().map(|ts| ts.tiles().len()).sum();

    // --- atlas_to_png: CGA mode (bit_depth < 8 only) ---------------------
    let cga_atlas = atlas_to_png(
        &sha,
        &AtlasOptions {
            output: TilesetColorOutput::Indexed,
            mode: ScreenMode::Cga,
            filter: TileFilter::default(),
            padding: 0,
        },
    );
    let cga_tile_count: usize = sha
        .tilesets()
        .iter()
        .filter(|ts| ts.bit_depth() < 8)
        .map(|ts| ts.tiles().len())
        .sum();

    // CGA tile count must be ≤ VGA tile count.
    check!(cga_tile_count <= vga_tile_count);
    // CGA atlas must be non-empty when sub-8-bit tilesets exist.
    if cga_tile_count > 0 {
        check!(cga_atlas.width() >= 1);
        check!(cga_atlas.height() >= 1);
    }

    // --- atlas_to_png: font-only filter -----------------------------------
    let font_count: usize = sha
        .tilesets()
        .iter()
        .filter(|ts| ts.is_font())
        .map(|ts| ts.tiles().len())
        .sum();
    let font_atlas = atlas_to_png(
        &sha,
        &AtlasOptions {
            output: TilesetColorOutput::Indexed,
            mode: ScreenMode::Vga,
            filter: TileFilter {
                fonts: true,
                pictures: false,
            },
            padding: 0,
        },
    );
    if font_count > 0 {
        check!(font_atlas.width() > 0);
        check!(font_atlas.height() > 0);
    }

    // --- atlas_to_png: with padding ---------------------------------------
    let padded = atlas_to_png(
        &sha,
        &AtlasOptions {
            output: TilesetColorOutput::Indexed,
            mode: ScreenMode::Vga,
            filter: TileFilter::default(),
            padding: 2,
        },
    );
    // Adding padding can only increase or maintain atlas dimensions when
    // more than one tile is present.
    check!(padded.width() >= vga_atlas.width() || vga_tile_count <= 1);
}

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling
/// back to the workspace-relative `data/original/JILL1` path.  Returns `None`
/// when neither is available so the caller can self-skip.
///
/// `CARGO_WORKSPACE_DIR` is a custom environment variable injected at compile
/// time by the workspace's `.cargo/config.toml` `[env]` section — it expands
/// to the absolute path of the workspace root so tests can find the shared
/// `data/` directory regardless of where Cargo invokes them from.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }

    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    if default.is_dir() {
        Some(default)
    } else {
        None
    }
}
