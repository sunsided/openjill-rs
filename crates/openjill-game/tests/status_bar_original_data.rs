//! Integration test that verifies the SHA tileset indices assumed by the static
//! status bar layout against the original `JILL1.SHA` when that file is locally
//! available. Self-skips cleanly when neither `OPENJILL_DATA_DIR` nor the
//! default `data/original/JILL1` path is present.

use assert2::check;
use openjill_core::RenderCommand;
use openjill_data::DataDirectory;
use openjill_render::ShaFontTiles;
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

/// Unit under test: font tileset decode from real `JILL1.SHA` plus `status_bar_commands`
/// `DrawText` emission for the documented status-bar labels.
///
/// Note: no framebuffer rendering happens here. The test asserts that the font tileset
/// can be located and decoded into `ShaFontTiles` without error, and that the
/// `RenderCommand` stream contains each expected label entry. The actual on-screen
/// rendering path is covered by the renderer crate's headless framebuffer tests.
///
/// Preconditions: either `OPENJILL_DATA_DIR` or the workspace `data/original/JILL1`
/// directory is present. When neither is available the test prints a skip message
/// and returns.
///
/// Invariants asserted:
/// - `JILL1.SHA` contains at least one font tileset.
/// - `ShaFontTiles::from_tileset` decodes that tileset successfully.
/// - `status_bar_commands` emits one `DrawText` for every expected label ("CONTROLS",
///   "INVENTORY", "Open Jill : Jungle") at the documented (x, y, color).
#[test]
fn status_bar_text_labels_resolve_against_original_font() {
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

    let font_tileset = cache
        .sha
        .tilesets()
        .iter()
        .find(|ts| ts.is_font())
        .expect("JILL1.SHA must contain at least one font tileset for status-bar labels");
    let _font = ShaFontTiles::from_tileset(font_tileset)
        .expect("font tileset from JILL1.SHA must decode without error");

    let commands = openjill_game::status_bar::status_bar_commands();
    let labels: Vec<(&str, i32, i32, u8)> = commands
        .iter()
        .filter_map(|cmd| match cmd {
            RenderCommand::DrawText {
                text,
                x,
                y,
                color_index,
                ..
            } => Some((text.as_str(), *x, *y, *color_index)),
            _ => None,
        })
        .collect();

    check!(
        labels.contains(&("CONTROLS", 10, 5, 1)),
        "status bar must emit CONTROLS DrawText at (10, 5) color 1"
    );
    check!(
        labels.contains(&("INVENTORY", 13, 179, 1)),
        "status bar must emit INVENTORY DrawText at (13, 179) color 1"
    );
    check!(
        labels.contains(&("Open Jill : Jungle", 129, 4, 1)),
        "status bar must emit Open Jill : Jungle DrawText at (129, 4) color 1"
    );
}
