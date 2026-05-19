//! Integration test that parses the original `JILL1.VCL` text-entry table
//! when the game's data directory is available locally. The test self-skips
//! when the data is not present so CI runs without copyrighted bytes still
//! pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::vcl::VclFile;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: end-to-end parsing of the original `JILL1.VCL` file via
/// [`DataDirectory::open_reader`] + [`VclFile::parse`].
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL1.VCL`, or the workspace-relative `data/original/JILL1` directory is
/// present. When neither is available the test prints a skip message and
/// returns `Ok(())` so machines without the original data still pass CI.
///
/// Invariants asserted: parsing succeeds, the parsed file exposes at least
/// one non-empty text entry, `text_entry_count` agrees with
/// `text_entries().len()`, every entry's preserved offset points inside the
/// source file, and every entry's text is non-empty.
#[test]
fn parses_original_jill_vcl_text_entries_when_available() {
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
    let mut reader = directory.open_reader("JILL1.VCL").unwrap_or_else(|error| {
        panic!(
            "JILL1.VCL must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let file_len = reader.len();

    let vcl = VclFile::parse(&mut reader).expect("JILL1.VCL from original data should parse");
    check!(
        !vcl.text_entries().is_empty(),
        "JILL1.VCL should contain non-empty text entries"
    );
    check!(vcl.text_entry_count() == vcl.text_entries().len());

    for entry in vcl.text_entries() {
        check!(
            entry.offset() < file_len,
            "entry offset must point into file: {}",
            entry.offset()
        );
        check!(
            !entry.text().is_empty(),
            "parsed text entries should be non-empty"
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
