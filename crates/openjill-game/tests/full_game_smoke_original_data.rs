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

use openjill_core::{ActiveInput, InputCommand, RenderCommand, ScreenTransition};
use openjill_data::DataDirectory;
use openjill_data::episode;
use openjill_game::orchestrator::GameOrchestrator;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

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

    // Confirm the default selection ("play") to enter the world map.
    orchestrator.tick(&input(InputCommand::ThrowItem));
    for _ in 0..30 {
        let commands = orchestrator.tick(&ActiveInput::new());
        assert!(!commands.is_empty(), "world map must render a frame");
    }

    // Enter level 1 directly (deterministic; map navigation is out of scope for
    // a smoke test) and run it, alternating walking right with idle ticks.
    orchestrator.force_transition(ScreenTransition::Level {
        file: String::from("1.JN1"),
        number: 1,
    });
    let walk_right = input(InputCommand::MoveRight);
    let idle = ActiveInput::new();
    let mut saw_blit = false;
    for tick in 0..300 {
        let commands = orchestrator.tick(if tick % 20 < 10 { &walk_right } else { &idle });
        assert!(!commands.is_empty(), "level must render a frame");
        saw_blit |= commands
            .iter()
            .any(|command| matches!(command, RenderCommand::Blit { .. }));
        assert!(
            !orchestrator.is_quitting(),
            "the smoke run must not quit the game"
        );
    }

    assert!(
        saw_blit,
        "the level should render real content (at least one Blit)"
    );
}
