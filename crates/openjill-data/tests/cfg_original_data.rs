//! Integration test that parses the original `JILL1.CFG` data when the game's
//! data directory is available locally. The test self-skips when the data is
//! not present so CI runs without copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::cfg::CfgFile;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: end-to-end parsing of the original `JILL1.CFG` file via
/// [`DataDirectory::open_reader`] + [`CfgFile::parse`].
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL1.CFG`, or the workspace-relative `data/original/JILL1` directory is
/// present. When neither is available the test prints a skip message and
/// returns so machines without the original data still pass CI.
///
/// Invariants asserted: parsing succeeds, high-score and save-slot counts
/// match the expected table sizes, save-slot metadata uses the `JN1` prefix,
/// and every save/high-score name consists only of OpenJill-printable bytes.
#[test]
fn parses_original_jill_cfg_when_available() {
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
    check!(cfg.high_scores().len() == 10);
    check!(cfg.save_slots().len() == 6);
    check!(
        cfg.setup().display_mode() == 1
            || cfg.setup().display_mode() == 2
            || cfg.setup().display_mode() == 4
    );

    for (index, high_score) in cfg.high_scores().iter().enumerate() {
        check!(
            high_score
                .name()
                .bytes()
                .all(|byte| (32..=127).contains(&byte)),
            "high-score name bytes should be OpenJill-printable at slot {index}"
        );
    }

    for (index, slot) in cfg.save_slots().iter().enumerate() {
        check!(slot.save_game_file() == format!("JN1SAVE.{index}"));
        check!(slot.save_map_file() == format!("JN1SAVEM.{index}"));
        check!(
            slot.name().bytes().all(|byte| (32..=127).contains(&byte)),
            "save-slot name bytes should be OpenJill-printable at slot {index}"
        );
    }
}

/// Resolves the data directory used by the integration test, preferring an
/// explicit `OPENJILL_DATA_DIR` override and falling back to the workspace
/// default `data/original/JILL1` path. Returns `None` when neither is
/// available so the caller can self-skip.
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
