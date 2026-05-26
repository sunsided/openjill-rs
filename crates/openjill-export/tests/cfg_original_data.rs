//! Integration test for CFG high-score and save-slot text export against the
//! original `JILL1.CFG` data.
//!
//! The test self-skips when the original data is unavailable so CI remains green
//! on environments without copyrighted game assets.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::cfg::CfgFile;
use openjill_export::cfg::{save_slots_to_text, scores_to_text};
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: [`scores_to_text`] and [`save_slots_to_text`] against the
/// original `JILL1.CFG`.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a valid data directory
/// or the workspace-relative `data/original/JILL1` exists. If neither is
/// available, the test prints a skip message and returns.
///
/// Invariants asserted:
/// - `scores_to_text` emits a header row + divider + exactly 10 data rows.
/// - Each score row contains a 1-based rank, a name, and a score value.
/// - `save_slots_to_text` emits a header row + divider + exactly 6 data rows.
/// - Each slot row contains a 0-based slot index and `JN1SAVE.<index>`.
#[test]
fn exports_original_jill_cfg_high_scores_and_save_slots_when_available() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping integration test; {DATA_DIR_ENV} is not set and default data directory is missing"
            );
            return;
        }
    };

    check!(
        data_dir.is_dir(),
        "data directory must exist when configured: {}",
        data_dir.display()
    );

    let directory = DataDirectory::new(&data_dir);
    let mut reader = directory.open_reader("JILL1.CFG").unwrap_or_else(|error| {
        panic!(
            "JILL1.CFG must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });

    let cfg =
        CfgFile::parse(&mut reader, "JN1").expect("JILL1.CFG from original data should parse");

    // --- high-score table ---
    let scores_text = scores_to_text(&cfg);
    let score_lines: Vec<&str> = scores_text.lines().collect();

    // header + divider + 10 data rows
    check!(
        score_lines.len() == 12,
        "scores table must have 12 lines (header + divider + 10 rows)"
    );

    let header = score_lines[0];
    check!(header.contains("rank"), "header must contain 'rank'");
    check!(header.contains("name"), "header must contain 'name'");
    check!(header.contains("score"), "header must contain 'score'");
    check!(
        score_lines[1].contains("---"),
        "divider must contain dashes"
    );

    for (line_index, line) in score_lines.iter().skip(2).enumerate() {
        let rank = line_index + 1;
        let rank_str = rank.to_string();
        check!(
            line.split_whitespace().next() == Some(rank_str.as_str()),
            "row {rank} must start with rank number"
        );
    }

    // Rows are in source order: rank 1 is line index 2, rank 10 is line index 11.
    check!(
        score_lines[2].trim_start().starts_with('1')
            && !score_lines[2].trim_start().starts_with("10"),
        "rank 1 must be the first data row"
    );
    check!(
        score_lines[11].trim_start().starts_with("10"),
        "rank 10 must be the last data row"
    );

    // --- save-slot table ---
    let slots_text = save_slots_to_text(&cfg, "JN1");
    let slot_lines: Vec<&str> = slots_text.lines().collect();

    // header + divider + 6 data rows
    check!(
        slot_lines.len() == 8,
        "save-slots table must have 8 lines (header + divider + 6 rows)"
    );

    let slot_header = slot_lines[0];
    check!(slot_header.contains("slot"), "header must contain 'slot'");
    check!(slot_header.contains("name"), "header must contain 'name'");
    check!(
        slot_header.contains("save_game_file"),
        "header must contain 'save_game_file'"
    );
    check!(slot_lines[1].contains("---"), "divider must contain dashes");

    for (line_index, line) in slot_lines.iter().skip(2).enumerate() {
        let expected_file = format!("JN1SAVE.{line_index}");
        let slot_str = line_index.to_string();
        check!(
            line.split_whitespace().next() == Some(slot_str.as_str()),
            "slot {line_index} row must start with slot index {line_index}"
        );
        check!(
            line.contains(&expected_file),
            "slot {line_index} row must contain '{expected_file}'"
        );
    }
}

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling back
/// to the workspace-relative `data/original/JILL1` path. Returns `None` when
/// neither exists so the caller can self-skip.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }

    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    if default.is_dir() {
        Some(default)
    } else {
        None
    }
}
