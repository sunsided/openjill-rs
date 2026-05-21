//! Integration test for `MapScreen` using the original episode 1 game data.
//! Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::map_screen::MapScreen;
use std::path::{Path, PathBuf};

/// Environment variable for overriding the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace
/// fallback at `data/original/JILL1`.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        let buf = PathBuf::from(path);
        return Some(buf).filter(|p| p.is_dir());
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    Some(default).filter(|p| p.is_dir())
}

/// Unit under test: `MapScreen::from_bytes` and `MapScreen::tick` against the
/// real `MAP.JN1` and `JILL.DMA` files from episode 1.
///
/// Preconditions: either `OPENJILL_DATA_DIR` or the workspace
/// `data/original/JILL1` directory is present.  When neither is available
/// the test prints a skip message and returns without failing.
///
/// Invariants asserted:
/// - `AssetCache::load` and `MapScreen::from_bytes` succeed.
/// - An idle tick produces at least one `RenderCommand::Blit` (the real map
///   has many non-zero, non-transparent background cells).
/// - Pressing Escape (Pause) returns `ScreenTransition::StartMenu`.
/// - `MapScreen::map_jn_bytes` returns the original `MAP.JN1` byte buffer.
#[test]
fn map_screen_constructs_and_renders_with_original_data() {
    use openjill_core::RenderCommand;

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

    let directory = DataDirectory::new(&data_dir);
    let cache = AssetCache::load(&directory)
        .unwrap_or_else(|err| panic!("AssetCache::load should succeed with real data: {err}"));

    let map_path = directory
        .resolve_path_case_insensitive("MAP.JN1")
        .expect("MAP.JN1 should resolve in real data");
    let map_bytes = std::fs::read(&map_path).expect("MAP.JN1 should read");

    let mut screen =
        MapScreen::from_bytes(map_bytes.clone(), cache.dma.clone()).expect("MAP.JN1 should parse");

    let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
    check!(result.transition.is_none(), "idle tick must not transition");
    check!(
        result
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Blit { .. })),
        "idle tick must emit at least one Blit for background tiles"
    );

    // Escape returns to the start menu.
    let mut esc = ActiveInput::new();
    esc.insert(InputCommand::Pause);
    let esc_result = screen.tick(&esc, &mut RuntimeState::new());
    check!(
        esc_result.transition == Some(ScreenTransition::StartMenu),
        "Escape must return ScreenTransition::StartMenu"
    );

    // map_jn_bytes round-trips the source file.
    check!(
        screen.map_jn_bytes() == Some(map_bytes),
        "map_jn_bytes must return the original MAP.JN1 byte buffer"
    );
}
