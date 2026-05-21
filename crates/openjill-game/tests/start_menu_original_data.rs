//! Integration test for `StartMenuScreen` using the original episode 1 game
//! files. Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::start_menu::StartMenuScreen;
use std::path::{Path, PathBuf};

/// Environment variable for overriding the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace-relative fallback.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        let buf = PathBuf::from(path);
        return Some(buf).filter(|p| p.is_dir());
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    Some(default).filter(|p| p.is_dir())
}

/// Unit under test: `StartMenuScreen` constructed from real episode 1 data.
///
/// Preconditions: either `OPENJILL_DATA_DIR` or the workspace-relative
/// `data/original/JILL1` directory is present. When neither is available the
/// test prints a skip message and returns.
///
/// Invariants asserted:
/// - `AssetCache::load` and `StartMenuScreen::new` succeed.
/// - A first idle tick (no input) produces at least one `Blit` render command
///   (background tiles) and at least one `DrawText` command (menu title/items).
/// - Pressing `ThrowItem` with the default selection (item 0, "play") returns
///   `ScreenTransition::Map`.
/// - Pressing `Pause` (Escape) from the base menu returns
///   `ScreenTransition::Quit`.
#[test]
fn start_menu_constructs_and_renders_with_original_data() {
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

    let mut screen = StartMenuScreen::new(
        cache.intro_jn.clone(),
        cache.dma.clone(),
        cache.vcl.clone(),
        cache.cfg.clone(),
    );

    // Idle tick with no input.
    let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
    check!(result.transition.is_none(), "idle tick must not transition");
    check!(
        result
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::DrawText { .. })),
        "idle tick must emit at least one DrawText command for the menu"
    );
    check!(
        result
            .commands
            .iter()
            .any(|c| matches!(c, RenderCommand::Blit { .. })),
        "idle tick must emit at least one Blit command for the background"
    );

    // Confirming item 0 ("play") transitions to Map.
    let mut confirm = ActiveInput::new();
    confirm.insert(InputCommand::ThrowItem);
    let confirm_result = screen.tick(&confirm, &mut RuntimeState::new());
    check!(
        confirm_result.transition == Some(ScreenTransition::Map),
        "confirming 'play' must return ScreenTransition::Map"
    );

    // Escape (Pause) from the base menu quits the game.
    let mut screen2 = StartMenuScreen::new(
        cache.intro_jn.clone(),
        cache.dma.clone(),
        cache.vcl.clone(),
        cache.cfg.clone(),
    );
    let mut esc = ActiveInput::new();
    esc.insert(InputCommand::Pause);
    let quit_result = screen2.tick(&esc, &mut RuntimeState::new());
    check!(
        quit_result.transition == Some(ScreenTransition::Quit),
        "Escape from base menu must return ScreenTransition::Quit"
    );
}
