//! Integration test that exports text entries from `JILL1.VCL` when original
//! data is available locally. The test self-skips on environments without data.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::vcl::VclFile;
use openjill_export::vcl::{entries_to_json, entries_to_text, escape_text_payload};
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Verifies that `JILL1.VCL` entries from original data can be exported to
/// both text and JSON forms.
///
/// Unit under test: [`entries_to_text`] + [`entries_to_json`] on original
/// episode-1 `JILL1.VCL`.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a valid directory with
/// `JILL1.VCL`, or workspace-relative `data/original/JILL1` exists. When
/// neither is available this test self-skips.
///
/// Invariants asserted: text output has one line per parsed entry in table
/// order, each line stores a right-aligned index prefix and a control-character-
/// escaped payload; JSON output parses as an array with the same entry count and
/// matching `{index,payload}` values.
#[test]
fn exports_original_jill_vcl_entries_when_available() {
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
        "data directory must exist when configured"
    );

    let directory = DataDirectory::new(&data_dir);
    let mut reader = directory.open_reader("JILL1.VCL").unwrap_or_else(|error| {
        panic!(
            "JILL1.VCL must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });

    let vcl = VclFile::parse(&mut reader)
        .unwrap_or_else(|error| panic!("JILL1.VCL from original data should parse: {error}"));
    check!(!vcl.text_entries().is_empty());

    let text = entries_to_text(&vcl);
    let lines: Vec<&str> = text.lines().collect();
    check!(lines.len() == vcl.text_entry_count());

    let index_width = vcl
        .text_entries()
        .iter()
        .map(|entry| entry.index())
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(1);

    for (entry, line) in vcl.text_entries().iter().zip(lines.iter().copied()) {
        let (index_part, payload_part) = line
            .split_once(": ")
            .expect("text line should separate index and payload");
        check!(index_part.len() == index_width);
        check!(index_part.trim().parse::<usize>().ok() == Some(entry.index()));
        check!(payload_part == escape_text_payload(entry.text()));
    }

    let json: serde_json::Value =
        serde_json::from_str(&entries_to_json(&vcl)).expect("json output should parse");
    let entries = json.as_array().expect("json output should be an array");
    check!(entries.len() == vcl.text_entry_count());

    for (entry, json_entry) in vcl.text_entries().iter().zip(entries.iter()) {
        check!(json_entry["index"] == serde_json::Value::from(entry.index()));
        check!(json_entry["payload"] == serde_json::Value::from(entry.text()));
    }
}

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling
/// back to the workspace-relative `data/original/JILL1` path.
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
