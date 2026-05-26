//! Integration test for JN map PNG export against original episode-1 data.
//!
//! The test self-skips when original data is unavailable so CI remains green on
//! environments without copyrighted game assets.

use assert2::check;
use openjill_core::Palette;
use openjill_core::entity::Rect;
use openjill_data::DataDirectory;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_export::jn::{map_to_png, map_to_png_with_viewport};
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: [`map_to_png`] and [`map_to_png_with_viewport`] against
/// `MAP.JN1` from original episode-1 data.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a valid data directory,
/// or the workspace-relative `data/original/JILL1` exists. If neither is
/// available, the test prints a skip message and returns.
///
/// Invariants asserted: full map export renders at `128 × 64 × 16` pixels,
/// produces at least one non-transparent pixel, and viewport rendering returns
/// the requested clipped dimensions.
#[test]
fn renders_original_map_jn_to_expected_dimensions_when_available() {
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

    let mut jn_reader = directory.open_reader("MAP.JN1").unwrap_or_else(|error| {
        panic!(
            "MAP.JN1 must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let jn = JnFile::parse(&mut jn_reader).expect("MAP.JN1 from original data should parse");

    let mut dma_reader = directory.open_reader("JILL.DMA").unwrap_or_else(|error| {
        panic!(
            "JILL.DMA must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let dma = DmaFile::parse(&mut dma_reader).expect("JILL.DMA from original data should parse");

    let mut sha_reader = directory.open_reader("JILL1.SHA").unwrap_or_else(|error| {
        panic!(
            "JILL1.SHA must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let sha = ShaFile::parse(&mut sha_reader).expect("JILL1.SHA from original data should parse");

    let palette = Palette::jill_vga();
    let image = map_to_png(&jn, &sha, &dma, &palette);
    check!(image.width() == 2048);
    check!(image.height() == 1024);
    check!(
        image.pixels().any(|pixel| pixel.0[3] != 0),
        "rendered map should contain at least one opaque pixel"
    );

    let viewport = Rect::new(128, 64, 160, 96);
    let clipped = map_to_png_with_viewport(&jn, &sha, &dma, &palette, Some(viewport));
    check!(clipped.width() == 160);
    check!(clipped.height() == 96);
}

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling
/// back to the workspace-relative `data/original/JILL1` path. Returns `None`
/// when neither exists so the caller can self-skip.
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
