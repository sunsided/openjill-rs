//! Integration test for `MapScreen` using the original episode 1 game data.
//! Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_data::episode;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::map_screen::MapScreen;
use std::path::{Path, PathBuf};

/// SHA tileset that carries every player sprite frame (mirrors the
/// `PLAYER_TILESET` constant in `screens::jn_object_layer`).  Asserting on
/// this tileset distinguishes the player `Blit` from any background tile
/// `Blit` even when both happen to land at the same framebuffer position.
const PLAYER_TILESET: u8 = 8;

/// Environment variable for overriding the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

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
/// - The idle tick also emits at least one `Blit` from the player tileset
///   (`PLAYER_TILESET = 8`) whose framebuffer position lies inside the game
///   area, confirming the static `MAP.JN1` object layer drew Jill.
/// - Pressing Escape (Pause) returns `ScreenTransition::StartMenu`.
/// - `MapScreen::map_jn_bytes` returns the original `MAP.JN1` byte buffer.
#[test]
fn map_screen_constructs_and_renders_with_original_data() {
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
    let cache = AssetCache::load(&directory, &episode::JILL1)
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

    // The MAP.JN1 object layer should now contribute Jill's stand pose.
    let game_area_right = GAME_AREA_X + GAME_AREA_W as i32;
    let game_area_bottom = GAME_AREA_Y + GAME_AREA_H as i32;
    check!(
        result.commands.iter().any(|c| match c {
            RenderCommand::Blit { tileset, x, y, .. } => {
                *tileset == PLAYER_TILESET
                    && *x >= GAME_AREA_X
                    && *x < game_area_right
                    && *y >= GAME_AREA_Y
                    && *y < game_area_bottom
            }
            _ => false,
        }),
        "idle tick must emit a player-tileset Blit inside the game area for the MAP.JN1 starting Jill"
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
