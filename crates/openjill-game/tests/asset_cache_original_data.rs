//! Integration test that loads the full `AssetCache` from the original episode
//! 1 game files when those files are available locally. The test self-skips
//! when neither `OPENJILL_DATA_DIR` nor the default `data/original/JILL1` path
//! is present, so CI runs without the copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_game::asset_cache::AssetCache;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace-relative
/// fallback path. Returns `None` when neither location is available.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }
    let fallback = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .map(|workspace_root| workspace_root.join("data/original/JILL1"));
    fallback.filter(|p| p.is_dir())
}

/// Unit under test: `AssetCache::load` with real episode 1 game files.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL.DMA`, `JILL1.SHA`, `JILL1.VCL`, and `JILL1.CFG`, or the
/// workspace-relative `data/original/JILL1` directory is present. When neither
/// is available the test prints a skip message and returns so CI still passes.
///
/// Invariants asserted: `AssetCache::load` succeeds; the loaded DMA file has
/// at least one entry; the SHA file has at least one tileset; the VCL file and
/// CFG file load without error.
#[test]
fn loads_all_assets_from_original_episode_1_data() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is not set \
                 and default data directory is missing"
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
    let cache = AssetCache::load(&directory)
        .unwrap_or_else(|err| panic!("AssetCache::load should succeed with real data: {err}"));

    check!(
        !cache.dma.entries().is_empty(),
        "JILL.DMA should have at least one entry"
    );
    check!(
        !cache.sha.tilesets().is_empty(),
        "JILL1.SHA should have at least one tileset"
    );
}
