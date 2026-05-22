//! Integration test for `StartMenuScreen` using the original episode 1 game
//! files. Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is available.

use assert2::check;
use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_game::asset_cache::AssetCache;
use openjill_game::screens::start_menu::StartMenuScreen;
use std::path::{Path, PathBuf};

/// Framebuffer X of the start-menu box's top-left corner (from
/// `start_menu.json` `x` = 72).
const MENU_BOX_X: i32 = 72;

/// Framebuffer Y of the start-menu box's top-left corner (from
/// `start_menu.json` `y` = 64).
const MENU_BOX_Y: i32 = 64;

/// Conservative right edge of the start-menu box in framebuffer pixels.
///
/// The actual right edge derives from the title / item text width, but
/// the menu box is known to fit inside `[MENU_BOX_X, MENU_BOX_X + 144]`
/// for the shipped `start_menu.json` (15-character title plus the
/// 8-pixel-wide corner / bar frame tiles).  This bound is used by the
/// integration test to assert that the Jill stand-pose `Blit` lands
/// strictly to the right of the menu box.
const MENU_BOX_RIGHT_LIMIT: i32 = MENU_BOX_X + 144;

/// Conservative bottom edge of the start-menu box in framebuffer
/// pixels.  Mirrors [`MENU_BOX_RIGHT_LIMIT`]: nine items plus three
/// non-item rows at the 8-pixel small-font row height is well within
/// 112 pixels.
const MENU_BOX_BOTTOM_LIMIT: i32 = MENU_BOX_Y + 112;

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
///   (object index 61, world (1968, 1000)) and at least one `Blit` whose
///   framebuffer position lies inside the game area but outside the
///   start-menu box (the Jill stand-pose sprite at world (1960, 944),
///   framebuffer `(GAME_AREA_X + 152, GAME_AREA_Y + 80)`).
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

    // The INTRO.JN1 object layer must contribute the shareware notice
    // text and the Jill stand-pose sprite to the same tick.
    let game_area_right = GAME_AREA_X + GAME_AREA_W as i32;
    let game_area_bottom = GAME_AREA_Y + GAME_AREA_H as i32;

    check!(
        result.commands.iter().any(|c| match c {
            RenderCommand::DrawText { text, .. } => text.contains("SHAREWARE"),
            _ => false,
        }),
        "idle tick must emit a DrawText carrying the shareware notice"
    );

    check!(
        result.commands.iter().any(|c| match c {
            RenderCommand::Blit { x, y, .. } => {
                let inside_game_area = *x >= GAME_AREA_X
                    && *x < game_area_right
                    && *y >= GAME_AREA_Y
                    && *y < game_area_bottom;
                let outside_menu_box = *x >= MENU_BOX_RIGHT_LIMIT || *y >= MENU_BOX_BOTTOM_LIMIT;
                inside_game_area && outside_menu_box
            }
            _ => false,
        }),
        "idle tick must emit a Blit for the Jill stand-pose inside the game area but outside the menu box"
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
