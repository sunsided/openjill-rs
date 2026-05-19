//! Integration test that parses the original `JILL1.SHA` file when the game's
//! data directory is available locally. The test self-skips when the data is
//! not present so CI runs without copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::sha::ShaFile;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: end-to-end parsing of the original `JILL1.SHA` file via
/// [`DataDirectory::open_reader`] + [`ShaFile::parse`].
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL1.SHA`, or the workspace-relative `data/original/JILL1` directory is
/// present. When neither is available the test prints a skip message and
/// returns so machines without the original data still pass CI.
///
/// Invariants asserted: parsing succeeds, the header retains all 128 entries,
/// the number of parsed tilesets matches the number of valid header entries,
/// every parsed tileset corresponds to a valid header entry and preserves its
/// header offset/size, the optional color map follows the
/// `!is_font() && bit_depth() < 8` rule, and every tile has indexed-pixel data
/// whose length matches `width * height` and whose source offset points into
/// the file.
#[test]
fn parses_original_jill_sha_when_available() {
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
        "data directory must exist when configured: {}",
        data_dir.display()
    );

    let directory = DataDirectory::new(&data_dir);
    let mut reader = directory.open_reader("JILL1.SHA").unwrap_or_else(|error| {
        panic!(
            "JILL1.SHA must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let file_len = reader.len();

    let sha = ShaFile::parse(&mut reader).expect("JILL1.SHA from original data should parse");
    check!(sha.header().entries().len() == 128);
    check!(
        !sha.tilesets().is_empty(),
        "JILL1.SHA should contain at least one valid tileset"
    );
    check!(sha.tilesets().len() == sha.header().valid_entry_count());

    for tileset in sha.tilesets() {
        let header_entry = sha
            .header()
            .entry(tileset.entry_index())
            .expect("tileset entry index must exist in header");
        check!(
            header_entry.is_valid(),
            "parsed tilesets must come from valid header entries"
        );
        check!(tileset.offset() == header_entry.offset());
        check!(tileset.size() == header_entry.size());
        check!((tileset.offset() as usize) < file_len);
        check!(tileset.tile_count() as usize == tileset.tiles().len());

        match tileset.color_map() {
            Some(color_map) => {
                check!(
                    !tileset.is_font(),
                    "font tilesets should not expose color maps"
                );
                check!(
                    tileset.bit_depth() < 8,
                    "color maps only exist below 8 bits"
                );
                check!(color_map.len() == (1usize << tileset.bit_depth()));
            }
            None => {
                check!(
                    tileset.is_font() || tileset.bit_depth() >= 8,
                    "missing color map should only happen for fonts or 8-bit tilesets"
                );
            }
        }

        for tile in tileset.tiles() {
            let expected_pixel_count = usize::from(tile.width()) * usize::from(tile.height());
            check!(
                tile.offset() < file_len,
                "tile pixel offset must point into file"
            );
            check!(tile.tileset_index() == tileset.entry_index());
            check!(tile.indexed_pixels().len() == expected_pixel_count);
        }
    }
}

/// Resolves the data directory used by the integration test, preferring an
/// explicit `OPENJILL_DATA_DIR` override and falling back to the workspace
/// default `data/original/JILL1` path. Returns `None` when neither is
/// available so the caller can self-skip.
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
