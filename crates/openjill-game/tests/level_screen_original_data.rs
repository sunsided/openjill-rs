//! Integration test for `LevelScreen` using the original episode 1 game data.
//! Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, MessageDispatcher, ScreenHandler};
use openjill_data::DataDirectory;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::level_screen::LevelScreen;
use std::path::{Path, PathBuf};

/// Environment variable for overriding the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// First playable level file in episode 1.
///
/// Episode 1 level files are named `<level_number>.JN1` on disk
/// (`1.JN1`, `2.JN1`, ..., `50.JN1`).
const LEVEL_FILE: &str = "1.JN1";

/// Outcome of resolving the data directory location.
///
/// Distinguishes a missing-default skip from an explicit-but-invalid
/// `OPENJILL_DATA_DIR` so the skip log line tells the developer which case
/// they are in.
enum DataDirOutcome {
    /// A usable data directory was found at the contained path.
    Found(PathBuf),
    /// `OPENJILL_DATA_DIR` was set but the supplied path is not a directory.
    EnvSetButNotDirectory(PathBuf),
    /// `OPENJILL_DATA_DIR` was unset and the workspace default is absent.
    UnsetAndDefaultMissing(PathBuf),
}

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace
/// fallback at `data/original/JILL1`, returning the cause when no usable
/// directory is found.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> DataDirOutcome {
    if let Some(path) = env_override {
        let buf = PathBuf::from(path);
        if buf.is_dir() {
            return DataDirOutcome::Found(buf);
        }
        return DataDirOutcome::EnvSetButNotDirectory(buf);
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    if default.is_dir() {
        DataDirOutcome::Found(default)
    } else {
        DataDirOutcome::UnsetAndDefaultMissing(default)
    }
}

/// Unit under test: `LevelScreen::from_bytes` and `LevelScreen::tick` against
/// the real `1.JN1` and `JILL.DMA` files from episode 1.
///
/// Preconditions: either `OPENJILL_DATA_DIR` or the workspace
/// `data/original/JILL1` directory is present.  When neither is available
/// the test prints a skip message and returns without failing.
///
/// Invariants asserted:
/// - `AssetCache::load` and `LevelScreen::from_bytes` succeed.
/// - An idle tick produces at least one `RenderCommand::Blit` (the real
///   level has many non-zero, non-transparent background cells).
/// - The idle tick produces no `ScreenTransition`.
/// - `LevelScreen::level_jn_bytes` round-trips the source file bytes.
#[test]
fn level_screen_constructs_and_renders_with_original_data() {
    use openjill_core::RenderCommand;

    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        DataDirOutcome::Found(dir) => dir,
        DataDirOutcome::EnvSetButNotDirectory(path) => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is set to '{}' \
                 which is not a directory",
                path.display()
            );
            return;
        }
        DataDirOutcome::UnsetAndDefaultMissing(default) => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is not set and \
                 default '{}' is missing",
                default.display()
            );
            return;
        }
    };

    let directory = DataDirectory::new(&data_dir);
    let cache = AssetCache::load(&directory)
        .unwrap_or_else(|err| panic!("AssetCache::load should succeed with real data: {err}"));

    let level_path = match directory.resolve_path_case_insensitive(LEVEL_FILE) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("skipping; {LEVEL_FILE} not found in data dir: {err}");
            return;
        }
    };
    let level_bytes =
        std::fs::read(&level_path).unwrap_or_else(|err| panic!("{LEVEL_FILE} should read: {err}"));

    let mut dispatcher = MessageDispatcher::new();
    let mut screen =
        LevelScreen::from_bytes(level_bytes.clone(), cache.dma.clone(), 1, &mut dispatcher)
            .unwrap_or_else(|err| panic!("{LEVEL_FILE} should parse: {err}"));

    let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
    check!(result.transition.is_none(), "idle tick must not transition");
    check!(
        result
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Blit { .. })),
        "idle tick must emit at least one Blit for background tiles"
    );

    check!(
        screen.level_jn_bytes() == Some(level_bytes),
        "level_jn_bytes must return the original level byte buffer"
    );
}
