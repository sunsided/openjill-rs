//! Integration test for `StartMenuScreen` using the original episode 1 game
//! files. Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::layout::{GAME_AREA_X, GAME_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_data::episode;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::start_menu::StartMenuScreen;
use std::path::{Path, PathBuf};

/// `JILL1.SHA` tileset that owns every player sprite frame
/// (`PlayerStandConst.TILESET_INDEX = 8`).  Matching on this tileset
/// distinguishes the start-menu Jill `Blit` from the many background
/// tile blits emitted on the same tick.
const PLAYER_TILESET: u8 = 8;

/// Forward-facing stand tile inside [`PLAYER_TILESET`]
/// (`PlayerStandConst.TILE_MIDDLE_INDEX = 16`).
const PLAYER_TILE_STAND_MIDDLE: u16 = 16;

/// Framebuffer X the start menu's Jill stand-pose sprite must land at.
///
/// Derived from the start-menu viewport offset `(-1808, -864)` and the
/// `INTRO.JN1` player record's world coordinate `(1960, 944)`:
/// `GAME_AREA_X + (1960 - 1808) = GAME_AREA_X + 152`.
const JILL_BLIT_X: i32 = GAME_AREA_X + 152;

/// Framebuffer Y the start menu's Jill stand-pose sprite must land at.
///
/// Mirrors [`JILL_BLIT_X`]: `GAME_AREA_Y + (944 - 864) = GAME_AREA_Y + 80`.
const JILL_BLIT_Y: i32 = GAME_AREA_Y + 80;

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
/// - The first idle tick also emits the `INTRO.JN1` object layer: at least
///   one `DrawText` whose payload contains the substring `"SHAREWARE"`
///   (object index 61, world (1968, 1000)) and exactly one `Blit` of
///   `JILL1.SHA` tileset 8 tile 16 at framebuffer position
///   `(JILL_BLIT_X, JILL_BLIT_Y)` - the Jill stand-pose sprite from
///   `INTRO.JN1`'s type 0 player record at world (1960, 944).
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
    let cache = AssetCache::load(&directory, &episode::JILL1)
        .unwrap_or_else(|err| panic!("AssetCache::load should succeed with real data: {err}"));

    let mut screen = StartMenuScreen::new(
        cache.intro_jn.clone(),
        episode::JILL1.intro_jn(),
        cache.dma.clone(),
        cache.vcl.clone(),
        cache.cfg.clone(),
        &cache.sha,
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

    // The INTRO.JN1 object layer must contribute the shareware notice
    // text and the Jill stand-pose sprite to the same tick.
    check!(
        result.commands.iter().any(|c| match c {
            RenderCommand::DrawText { text, .. } => text.contains("SHAREWARE"),
            _ => false,
        }),
        "idle tick must emit a DrawText carrying the shareware notice"
    );

    // Match the exact Jill stand-pose Blit: player tileset 8, forward-
    // facing stand tile 16, and the world-to-framebuffer position the
    // start-menu viewport offset predicts.  A looser predicate would
    // match background tile blits inside the game area and could pass
    // even if the JN object layer never drew Jill.
    check!(
        result.commands.iter().any(|c| match c {
            RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                ..
            } => {
                *tileset == PLAYER_TILESET
                    && *tile == PLAYER_TILE_STAND_MIDDLE
                    && *x == JILL_BLIT_X
                    && *y == JILL_BLIT_Y
            }
            _ => false,
        }),
        "idle tick must emit the Jill stand-pose Blit (tileset 8 tile 16) at (JILL_BLIT_X, JILL_BLIT_Y)"
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
        episode::JILL1.intro_jn(),
        cache.dma.clone(),
        cache.vcl.clone(),
        cache.cfg.clone(),
        &cache.sha,
    );
    let mut esc = ActiveInput::new();
    esc.insert(InputCommand::Pause);
    let quit_result = screen2.tick(&esc, &mut RuntimeState::new());
    check!(
        quit_result.transition == Some(ScreenTransition::Quit),
        "Escape from base menu must return ScreenTransition::Quit"
    );
}
