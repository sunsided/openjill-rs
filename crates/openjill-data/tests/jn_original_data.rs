//! Integration test that parses original episode 1 `*.JN1` data when the
//! game's data directory is available locally. The test self-skips when the
//! data is not present so CI runs without copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::jn::{BACKGROUND_CELL_COUNT, BACKGROUND_MAP_CODE_MASK, JnFile};
use std::fs;
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";
/// Required episode 1 JN files that must be covered when real data is present.
const REQUIRED_JN_FILES: &[&str] = &["INTRO.JN1", "MAP.JN1"];

/// Unit under test: end-to-end parsing of original episode 1 JN files via
/// [`DataDirectory::open_reader`] + [`JnFile::parse`].
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `INTRO.JN1` and `MAP.JN1`, or the workspace-relative `data/original/JILL1`
/// directory is present. When neither is available the test prints a skip
/// message and returns `Ok(())` so machines without original data still pass
/// CI.
///
/// Invariants asserted: parsing succeeds, background dimensions are fixed,
/// all background map codes are masked, object indexes match their source
/// order, offsets point inside the file, save-data offsets point inside the
/// file, and string-stack offsets increase monotonically.
#[test]
fn parses_original_episode_one_jn_files_when_available() {
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

    let mut file_names = discover_jn_files(&data_dir);
    file_names.sort();
    check!(
        !file_names.is_empty(),
        "data directory should contain episode 1 JN files"
    );
    for required_file in REQUIRED_JN_FILES {
        check!(
            file_names
                .iter()
                .any(|file_name| file_name.eq_ignore_ascii_case(required_file)),
            "data directory should contain required JN file {required_file}"
        );
    }

    let directory = DataDirectory::new(&data_dir);

    for file_name in file_names {
        let mut reader = directory.open_reader(&file_name).unwrap_or_else(|error| {
            panic!(
                "{file_name} must be readable from configured data directory {}: {error}",
                data_dir.display()
            )
        });
        let file_len = reader.len();
        let jn = JnFile::parse(&mut reader)
            .unwrap_or_else(|error| panic!("{file_name} from original data should parse: {error}"));

        check!(jn.background().width() == 128);
        check!(jn.background().height() == 64);
        check!(jn.background().map_codes().len() == BACKGROUND_CELL_COUNT);
        check!(
            jn.background()
                .map_codes()
                .iter()
                .all(|map_code| map_code & !BACKGROUND_MAP_CODE_MASK == 0),
            "{file_name} background map codes must be masked"
        );

        for (index, object) in jn.objects().iter().enumerate() {
            check!(
                object.index() == index,
                "{file_name} object index should be preserved"
            );
            check!(
                object.offset() < file_len,
                "{file_name} object offset must point into file"
            );
            if let Some(string_index) = object.string_index() {
                check!(
                    string_index < jn.strings().len(),
                    "{file_name} string association must point at a parsed string"
                );
            }
        }

        check!(jn.save_data().offset() < file_len);
        check!(jn.save_data().hole().len() == 28);

        for window in jn.strings().windows(2) {
            check!(
                window[0].offset() < window[1].offset(),
                "{file_name} string offsets should increase monotonically"
            );
        }

        // `to_bytes` emits the canonical (masked-background) save form, so it
        // need not equal a distribution file byte-for-byte; it must, however,
        // be a stable fixed point: re-parsing the serialized bytes yields an
        // identical `JnFile` (background, objects, save-data, strings, and
        // offsets all reproduced).
        let reparsed = JnFile::from_bytes(jn.to_bytes()).unwrap_or_else(|error| {
            panic!("{file_name} re-serialized bytes should re-parse: {error}")
        });
        check!(
            reparsed == jn,
            "{file_name} parse -> to_bytes -> parse must be stable"
        );
    }
}

/// Discovers all `*.JN1` files directly inside `data_dir`.
fn discover_jn_files(data_dir: &Path) -> Vec<String> {
    fs::read_dir(data_dir)
        .expect("data directory should be readable")
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|file_name| {
            Path::new(file_name)
                .extension()
                .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("JN1"))
        })
        .collect()
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
