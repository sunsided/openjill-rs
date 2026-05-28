//! Integration test that runs the `openjill-vcl-extract` binary against the
//! original `JILL1.VCL` when the game data is available locally. Self-skips
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

/// Resolves `JILL1.VCL`, returning `None` to self-skip when absent.
fn vcl_path() -> Option<PathBuf> {
    let vcl = resolve_data_dir()?.join("JILL1.VCL");
    vcl.is_file().then_some(vcl)
}

/// Runs the binary with `args`, returning captured stdout.
fn run(vcl: &Path, args: &[&str]) -> (bool, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_openjill-vcl-extract"))
        .arg("--file")
        .arg(vcl)
        .args(args)
        .output()
        .expect("vcl-extract binary should run");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
    )
}

#[test]
fn text_dump_lists_all_entries() {
    let Some(vcl) = vcl_path() else {
        eprintln!("skipping: JILL1.VCL not found");
        return;
    };
    let (ok, text) = run(&vcl, &[]);
    assert!(ok, "text dump should exit successfully");
    let entry_lines = text
        .lines()
        .filter(|line| {
            line.split_once(':')
                .is_some_and(|(head, _)| head.trim().parse::<usize>().is_ok())
        })
        .count();
    assert!(entry_lines > 0, "text dump should contain indexed entries");
}

#[test]
fn json_dump_parses_as_array_of_entries() {
    let Some(vcl) = vcl_path() else {
        eprintln!("skipping: JILL1.VCL not found");
        return;
    };
    let (ok, text) = run(&vcl, &["--json"]);
    assert!(ok, "json dump should exit successfully");
    let value: serde_json::Value =
        serde_json::from_str(&text).expect("output should be valid JSON");
    let array = value.as_array().expect("top-level JSON should be an array");
    assert!(!array.is_empty(), "VCL should contain text entries");
    assert!(array[0].get("index").is_some(), "entries carry an index");
    assert!(array[0].get("payload").is_some(), "entries carry a payload");
}

#[test]
fn single_entry_matches_full_dump() {
    let Some(vcl) = vcl_path() else {
        eprintln!("skipping: JILL1.VCL not found");
        return;
    };
    // Find the first entry's index from the full JSON dump, then request it.
    let (_, full) = run(&vcl, &["--json"]);
    let array: serde_json::Value = serde_json::from_str(&full).expect("valid JSON");
    let first_index = array[0]["index"]
        .as_u64()
        .expect("first entry has an index");

    let (ok, single) = run(&vcl, &["--entry", &first_index.to_string(), "--json"]);
    assert!(ok, "single-entry dump should exit successfully");
    let entry: serde_json::Value = serde_json::from_str(&single).expect("valid JSON");
    assert_eq!(entry["index"].as_u64(), Some(first_index));
    assert_eq!(entry["payload"], array[0]["payload"]);
}

#[test]
fn out_of_range_entry_fails() {
    let Some(vcl) = vcl_path() else {
        eprintln!("skipping: JILL1.VCL not found");
        return;
    };
    let (ok, _) = run(&vcl, &["--entry", "99999"]);
    assert!(!ok, "an out-of-range entry index should fail");
}
