//! Integration tests that run the CLI dump commands against original game data
//! when the data directory is available locally. The tests self-skip when the
//! data is not present so CI runs without copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";
/// Number of hexadecimal characters in a SHA-256 digest.
const SHA256_HEX_LEN: usize = 64;
/// Number of text entries exposed by the VCL parser.
const VCL_TEXT_ENTRY_COUNT: usize = 40;

/// Unit under test: end-to-end `openjill-rs dump dma` and
/// `openjill-rs dump vcl` execution against original episode-1 files.
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL.DMA` and `JILL1.VCL`, or the workspace-relative
/// `data/original/JILL1` directory is present. When neither is available the
/// test prints a skip message and returns so machines without original data
/// still pass CI.
///
/// Invariants asserted: both dump commands succeed, write JSON outside the
/// original data tree, preserve source metadata, report non-empty entry arrays,
/// keep entry counts consistent with array lengths, and emit per-entry offsets
/// and table indexes that stay within the source-file structures.
#[test]
fn dumps_original_dma_and_vcl_payloads_when_available() {
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

    let directory = DataDirectory::new(&data_dir);
    let dma_len = directory
        .open_reader("JILL.DMA")
        .expect("JILL.DMA must be readable through DataDirectory")
        .len();
    let vcl_len = directory
        .open_reader("JILL1.VCL")
        .expect("JILL1.VCL must be readable through DataDirectory")
        .len();
    let temp_dir = TempDirGuard::new("openjill-cli-dump-original-data");
    let dma_output = temp_dir.path().join("dma.json");
    let vcl_output = temp_dir.path().join("vcl-text.json");

    run_dump_command("dma", &data_dir, &dma_output);
    run_dump_command("vcl", &data_dir, &vcl_output);

    let dma = read_json_file(&dma_output);
    let vcl = read_json_file(&vcl_output);

    assert_dma_dump(&dma, dma_len);
    assert_vcl_dump(&vcl, vcl_len);
}

/// Runs one CLI dump command and panics with captured output on failure.
fn run_dump_command(kind: &str, data_dir: &Path, output: &Path) {
    let command_output = Command::new(env!("CARGO_BIN_EXE_openjill-rs"))
        .args([
            "dump",
            kind,
            "--data-dir",
            data_dir.to_string_lossy().as_ref(),
            "--output",
            output.to_string_lossy().as_ref(),
            "--force",
        ])
        .output()
        .expect("dump command should start");

    if !command_output.status.success() {
        panic!(
            "dump {kind} failed with status {}\nstdout:\n{}\nstderr:\n{}",
            command_output.status,
            String::from_utf8_lossy(&command_output.stdout),
            String::from_utf8_lossy(&command_output.stderr)
        );
    }
}

/// Asserts structural invariants for a DMA dump JSON document.
fn assert_dma_dump(json: &Value, source_len: usize) {
    check!(json["source_file"] == Value::from("JILL.DMA"));
    check!(json["source_size"] == Value::from(source_len));
    assert_sha256_hex(&json["source_sha256"]);

    let entries = json["entries"]
        .as_array()
        .expect("DMA entries must be an array");
    let entry_count = json["entry_count"]
        .as_u64()
        .expect("DMA entry_count must be numeric") as usize;

    check!(entry_count == entries.len());
    check!(!entries.is_empty(), "JILL.DMA dump should contain entries");

    for (expected_index, entry) in entries.iter().enumerate() {
        check!(json_usize(entry, "index") == expected_index);
        check!(json_usize(entry, "source_offset") < source_len);
        assert_u16_hex(entry, "map_code", "map_code_hex");
        assert_u16_hex(entry, "flags", "flags_hex");
        check!(entry["tile"].as_u64().is_some());
        check!(entry["tileset"].as_u64().is_some());
        check!(entry["is_msg_touch"].as_bool().is_some());
        check!(entry["is_msg_draw"].as_bool().is_some());
        check!(entry["is_msg_update"].as_bool().is_some());
        check!(entry["is_player_thru"].as_bool().is_some());
        check!(entry["is_stair"].as_bool().is_some());
        check!(entry["is_vine"].as_bool().is_some());
        check!(
            !entry["name"]
                .as_str()
                .expect("DMA name must be a string")
                .is_empty()
        );
    }
}

/// Asserts structural invariants for a VCL dump JSON document.
fn assert_vcl_dump(json: &Value, source_len: usize) {
    check!(json["source_file"] == Value::from("JILL1.VCL"));
    check!(json["source_size"] == Value::from(source_len));
    check!(json["sound_entries_supported"] == Value::from(false));
    assert_sha256_hex(&json["source_sha256"]);

    let entries = json["entries"]
        .as_array()
        .expect("VCL entries must be an array");
    let text_entry_count = json["text_entry_count"]
        .as_u64()
        .expect("VCL text_entry_count must be numeric") as usize;

    check!(text_entry_count == entries.len());
    check!(
        !entries.is_empty(),
        "JILL1.VCL dump should contain text entries"
    );

    let mut previous_index = None;
    for entry in entries {
        let index = json_usize(entry, "index");
        let source_offset = json_usize(entry, "source_offset");
        let declared_length = json_usize(entry, "declared_length");
        let text = entry["text"].as_str().expect("VCL text must be a string");

        check!(index < VCL_TEXT_ENTRY_COUNT);
        if let Some(previous_index) = previous_index {
            check!(
                previous_index < index,
                "VCL entries must stay in table order"
            );
        }
        previous_index = Some(index);

        check!(declared_length > 0);
        check!(text.chars().count() == declared_length);
        check!(source_offset < source_len);
        check!(source_offset + declared_length <= source_len);
    }
}

/// Asserts that a JSON value is a lowercase SHA-256 hexadecimal digest.
fn assert_sha256_hex(value: &Value) {
    let digest = value.as_str().expect("source_sha256 must be a string");
    check!(digest.len() == SHA256_HEX_LEN);
    check!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    check!(digest == digest.to_ascii_lowercase());
}

/// Asserts that a numeric `u16` field matches its lowercase hexadecimal helper field.
fn assert_u16_hex(entry: &Value, numeric_field: &str, hex_field: &str) {
    let value = entry[numeric_field]
        .as_u64()
        .unwrap_or_else(|| panic!("{numeric_field} must be numeric"));
    check!(value <= u16::MAX as u64);
    check!(entry[hex_field] == Value::from(format!("0x{value:04x}")));
}

/// Reads one unsigned integer field from a JSON object as `usize`.
fn json_usize(json: &Value, field: &str) -> usize {
    json[field]
        .as_u64()
        .unwrap_or_else(|| panic!("{field} must be numeric")) as usize
}

/// Reads and deserializes a JSON file written by the dump command.
fn read_json_file(path: &Path) -> Value {
    let bytes = fs::read(path).expect("read dump output");
    serde_json::from_slice(&bytes).expect("parse dump output json")
}

/// Resolves the data directory used by the integration test, preferring an
/// explicit `OPENJILL_DATA_DIR` override and falling back to the workspace
/// default `data/original/JILL1` path. Returns `None` when neither is
/// available so the caller can self-skip.
fn resolve_data_dir(env_override: Option<&OsStr>) -> Option<PathBuf> {
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

/// Owned temporary directory guard that removes its path on drop.
struct TempDirGuard {
    /// Temporary directory path removed during drop.
    path: PathBuf,
}

impl TempDirGuard {
    /// Creates a unique temporary directory with the supplied `prefix`.
    fn new(prefix: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp directory");
        Self { path }
    }

    /// Returns the temporary directory path.
    fn path(&self) -> &Path {
        self.path.as_path()
    }
}

impl Drop for TempDirGuard {
    /// Removes the temporary directory tree as a best-effort cleanup.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
