//! Integration test that verifies the SHA tileset indices assumed by the static
//! status bar layout against the original `JILL1.SHA` when that file is locally
//! available. Self-skips cleanly when neither `OPENJILL_DATA_DIR` nor the
//! default `data/original/JILL1` path is present.

use assert2::check;
use openjill_data::DataDirectory;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Expected SHA header entry index for the status bar tile mosaic.
///
/// All entries in `status_bar_vga.json` use tileset 3 (0-based header entry).
const STATUS_BAR_TILESET_INDEX: usize = 3;

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace-relative
/// fallback path. Returns `None` when neither location is an existing directory.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path)).filter(|p| p.is_dir());
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    Some(default).filter(|p| p.is_dir())
}

/// Unit under test: SHA tileset 3 presence in real `JILL1.SHA`.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL1.SHA`, or the workspace-relative `data/original/JILL1` directory is present.
/// When neither is available the test prints a skip message and returns.
///
/// Invariants asserted: the parsed SHA file contains a tileset whose `entry_index`
/// equals `STATUS_BAR_TILESET_INDEX` (3), confirming that the hardcoded tileset
/// reference in `status_bar_vga.json` is valid against the real game data.
#[test]
fn sha_tileset_3_exists_in_original_data() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is unset or invalid \
                 and default data directory is missing"
            );
            return;
        }
    };

    let directory = DataDirectory::new(&data_dir);
    let cache = openjill_game::asset_cache::AssetCache::load(&directory)
        .unwrap_or_else(|err| panic!("AssetCache::load should succeed with real data: {err}"));

    check!(
        cache
            .sha
            .tilesets()
            .iter()
            .any(|ts| ts.entry_index() == STATUS_BAR_TILESET_INDEX),
        "JILL1.SHA must contain a valid tileset at header entry index {STATUS_BAR_TILESET_INDEX} \
         (required by status_bar_vga.json)"
    );

    let status_bar_tileset = cache
        .sha
        .tilesets()
        .iter()
        .find(|ts| ts.entry_index() == STATUS_BAR_TILESET_INDEX)
        .expect("tileset 3 confirmed present above");

    check!(
        !status_bar_tileset.tiles().is_empty(),
        "SHA tileset {STATUS_BAR_TILESET_INDEX} must have at least one tile"
    );
}
