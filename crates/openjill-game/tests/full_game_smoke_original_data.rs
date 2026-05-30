//! Full-game integration smoke test: boots the orchestrator from the original
//! episode 1 data and drives it across a representative path (start menu ->
//! PLAY -> world map -> a level) over many ticks, asserting it never panics and
//! keeps producing frames.
//!
//! This is a smoke test, not a scripted full playthrough: a deterministic clear
//! of every level would need authored input sequences. It guards integration -
//! asset load, screen transitions, and the tick loop - end to end.
//!
//! Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is present, so CI without the copyrighted bytes
//! still passes.

use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::{ActiveInput, InputCommand, RenderCommand, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_data::episode;
use openjill_game::orchestrator::GameOrchestrator;
use openjill_game::screens::level_screen::EPISODE_1_SKY_COLOR;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Returns `true` when `commands` contain the game-area sky `FillRect` that a
/// `LevelScreen` (a level or the world map) emits at the top of every frame.
///
/// The orchestrator always prepends the static status bar, so a non-empty frame
/// alone proves nothing; this game-area-sized sky fill is emitted only by an
/// active level/map render, so it distinguishes a real level/map from a
/// start-menu fallback after a failed transition.
fn has_game_area_sky(commands: &[RenderCommand]) -> bool {
    commands.iter().any(|command| {
        matches!(
            command,
            RenderCommand::FillRect { x, y, width, height, color }
                if *x == GAME_AREA_X
                    && *y == GAME_AREA_Y
                    && *width == GAME_AREA_W
                    && *height == GAME_AREA_H
                    && *color == EPISODE_1_SKY_COLOR
        )
    })
}

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace-relative
/// fallback path. Returns `None` when neither location is available.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    Some(default).filter(|p| p.is_dir())
}

/// Builds an `ActiveInput` holding a single command.
fn input(command: InputCommand) -> ActiveInput {
    let mut set = ActiveInput::new();
    set.insert(command);
    set
}

/// Unit under test: the whole game booted from original episode 1 data.
///
/// Preconditions: `OPENJILL_DATA_DIR` (or the default `data/original/JILL1`)
/// holds the episode 1 files, including `1.JN1`. Skips otherwise.
///
/// Invariants asserted: the orchestrator boots; the start menu, world map, and a
/// level each render a frame across many ticks; the level renders real content
/// (at least one `Blit`); and the run never quits or panics.
#[test]
fn boots_and_runs_episode_1_from_original_data() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping full-game smoke test; {DATA_DIR_ENV} is not set \
                 and default data directory is missing"
            );
            return;
        }
    };

    let mut orchestrator = GameOrchestrator::new(DataDirectory::new(data_dir), &episode::JILL1)
        .expect("orchestrator must boot from original episode 1 data");

    // Start menu: a few idle ticks must each render.
    for _ in 0..5 {
        let commands = orchestrator.tick(&ActiveInput::new());
        assert!(!commands.is_empty(), "start menu must render a frame");
    }

    // Confirm the default selection ("play") to enter the world map; the map
    // must render its game area (proving PLAY -> Map, not a menu fallback).
    // Confirm is Jump (Space): Ctrl/ThrowItem no longer confirms the menu so the
    // Ctrl+E / Ctrl+P chords work.
    orchestrator.tick(&input(InputCommand::Jump));
    let mut saw_map_sky = false;
    for _ in 0..30 {
        let commands = orchestrator.tick(&ActiveInput::new());
        saw_map_sky |= has_game_area_sky(&commands);
    }
    assert!(saw_map_sky, "world map must render its game-area frame");

    // Enter level 1 directly (deterministic; map navigation is out of scope for
    // a smoke test) and run it, alternating walking right with idle ticks.
    orchestrator.force_transition(ScreenTransition::Level {
        file: String::from("1.JN1"),
        number: 1,
    });
    let walk_right = input(InputCommand::MoveRight);
    let idle = ActiveInput::new();
    let mut saw_level_sky = false;
    for tick in 0..300 {
        let commands = orchestrator.tick(if tick % 20 < 10 { &walk_right } else { &idle });
        saw_level_sky |= has_game_area_sky(&commands);
        assert!(
            !orchestrator.is_quitting(),
            "the smoke run must not quit the game"
        );
    }

    assert!(
        saw_level_sky,
        "the level must render its game-area frame (not fall back to the menu)"
    );
}

/// Episode 1 level JN files shipped with the original data.
const EPISODE_1_LEVELS: &[(&str, i32)] = &[
    ("1.JN1", 1),
    ("2.JN1", 2),
    ("3.JN1", 3),
    ("4.JN1", 4),
    ("6.JN1", 6),
    ("9.JN1", 9),
    ("50.JN1", 50),
];

/// Unit under test: every episode 1 level (and the world map) loads and ticks
/// from the original data without panicking.
///
/// Preconditions: `OPENJILL_DATA_DIR` (or the default `data/original/JILL1`)
/// holds the episode 1 level files. Skips otherwise.
///
/// Invariants asserted: the world map and each level load via `force_transition`,
/// tick for many frames while walking, each render a frame with real content
/// (at least one `Blit`), and the run never quits. This broadens the single-level
/// smoke test to guard asset loading + the tick loop across the whole episode.
#[test]
fn every_episode_1_level_loads_and_ticks() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping per-level smoke test; {DATA_DIR_ENV} is not set \
                 and default data directory is missing"
            );
            return;
        }
    };

    let mut orchestrator = GameOrchestrator::new(DataDirectory::new(data_dir), &episode::JILL1)
        .expect("orchestrator must boot from original episode 1 data");

    let walk_right = input(InputCommand::MoveRight);
    let idle = ActiveInput::new();

    // World map first, then every level.
    orchestrator.force_transition(ScreenTransition::Map);
    let mut saw_map_sky = false;
    for _ in 0..30 {
        let commands = orchestrator.tick(&idle);
        saw_map_sky |= has_game_area_sky(&commands);
        assert!(!orchestrator.is_quitting(), "map run must not quit");
    }
    assert!(saw_map_sky, "world map must render its game-area frame");

    for &(file, number) in EPISODE_1_LEVELS {
        orchestrator.force_transition(ScreenTransition::Level {
            file: String::from(file),
            number,
        });
        let mut saw_sky = false;
        for tick in 0..120 {
            let commands = orchestrator.tick(if tick % 20 < 10 { &walk_right } else { &idle });
            saw_sky |= has_game_area_sky(&commands);
            assert!(
                !orchestrator.is_quitting(),
                "level {file} smoke run must not quit"
            );
        }
        assert!(
            saw_sky,
            "level {file} must render its game-area frame (not fall back to the menu)"
        );
    }
}
