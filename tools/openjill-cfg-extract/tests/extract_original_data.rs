//! Integration test that runs the `openjill-cfg-extract` binary against the
//! original `JILL1.CFG` when the game data is available locally. Self-skips
//! when the data directory is missing so CI passes without copyrighted bytes.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment variable overriding the data directory at runtime.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling
/// back to the workspace `data/original/JILL1`. Returns `None` to self-skip.
fn resolve_data_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(DATA_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    default.is_dir().then_some(default)
}

/// Resolves `JILL1.CFG`, returning `None` to self-skip when absent.
fn cfg_path() -> Option<PathBuf> {
    let cfg = resolve_data_dir()?.join("JILL1.CFG");
    cfg.is_file().then_some(cfg)
}

/// Runs the binary with `args`, returning success flag and stdout.
fn run(cfg: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_openjill-cfg-extract"))
        .arg("--file")
        .arg(cfg)
        .args(args)
        .output()
        .expect("cfg-extract binary should run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn scores_section_lists_the_high_score_table() {
    let Some(cfg) = cfg_path() else {
        eprintln!("skipping: JILL1.CFG not found");
        return;
    };
    let (ok, text) = run(&cfg, &["--scores"]);
    assert!(ok, "scores dump should exit successfully");
    assert!(
        text.contains("rank"),
        "scores dump should have a rank header"
    );
    // The original episode-1 CFG ships the classic placeholder high scores.
    assert!(
        text.contains("epic") || text.contains("Look"),
        "scores dump should include the default high-score names"
    );
}

#[test]
fn saves_section_uses_episode_prefix() {
    let Some(cfg) = cfg_path() else {
        eprintln!("skipping: JILL1.CFG not found");
        return;
    };
    let (ok1, text1) = run(&cfg, &["--saves", "--episode", "jn1"]);
    assert!(ok1, "saves dump should exit successfully");
    assert!(
        text1.contains("JN1SAVE"),
        "episode 1 save files should use the JN1 prefix"
    );

    let (ok2, text2) = run(&cfg, &["--saves", "--episode", "jn2"]);
    assert!(ok2, "saves dump should exit successfully");
    assert!(
        text2.contains("JN2SAVE"),
        "episode 2 save files should use the JN2 prefix"
    );
}

#[test]
fn json_dump_has_both_sections() {
    let Some(cfg) = cfg_path() else {
        eprintln!("skipping: JILL1.CFG not found");
        return;
    };
    let (ok, text) = run(&cfg, &["--json"]);
    assert!(ok, "json dump should exit successfully");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("output should be valid JSON");
    assert!(
        value
            .get("high_scores")
            .and_then(|v| v.as_array())
            .is_some(),
        "json should carry a high_scores array"
    );
    assert!(
        value.get("save_slots").and_then(|v| v.as_array()).is_some(),
        "json should carry a save_slots array"
    );
}
