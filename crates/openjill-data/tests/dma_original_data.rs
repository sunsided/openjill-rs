//! Integration test that parses the original `JILL.DMA` file when the game's
//! data directory is available locally. The test self-skips when the data is
//! not present so CI runs without copyrighted bytes still pass.

use assert2::check;
use openjill_data::DataDirectory;
use openjill_data::dma::DmaFile;
use std::path::{Path, PathBuf};

/// Mask used to validate that parsed tileset values fit in their lower six
/// bits, mirroring the `TILESET_MASK` constant inside the parser.
const TILESET_MASK: u8 = 0x3f;
/// Environment variable that lets a developer override the data directory at
/// runtime (`OPENJILL_DATA_DIR=/path/to/JILL1`).
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Unit under test: end-to-end parsing of the original `JILL.DMA` file via
/// [`DataDirectory::open_reader`] + [`DmaFile::parse`].
///
/// Preconditions: either `OPENJILL_DATA_DIR` points at a directory containing
/// `JILL.DMA`, or the workspace-relative `data/original/JILL1` directory is
/// present. When neither is available the test prints a skip message and
/// returns `Ok(())` so machines without the original data still pass CI.
///
/// Invariants asserted: parsing succeeds, produces at least one entry,
/// `entry_count` agrees with `entries().len()`, every entry's index matches
/// its position, every entry's offset points inside the source file, every
/// tileset is masked to six bits, both lookup maps resolve back to the
/// original entries, and entry offsets increase monotonically across the
/// file (i.e. the parser advances strictly forward).
#[test]
fn parses_original_jill_dma_when_available() {
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
    let mut reader = directory.open_reader("JILL.DMA").unwrap_or_else(|error| {
        panic!(
            "JILL.DMA must be readable from configured data directory {}: {error}",
            data_dir.display()
        )
    });
    let file_len = reader.len();

    let dma = DmaFile::parse(&mut reader).expect("JILL.DMA from original data should parse");
    check!(!dma.entries().is_empty(), "JILL.DMA should contain entries");
    check!(dma.entry_count() == dma.entries().len());

    for (index, entry) in dma.entries().iter().enumerate() {
        check!(entry.index() == index, "entry index should be preserved");
        check!(
            entry.offset() < file_len,
            "entry offset must point into file"
        );
        check!(
            entry.tileset() & !TILESET_MASK == 0,
            "tileset must be masked to 6 bits"
        );

        check!(
            dma.get_by_map_code(entry.map_code()).is_some(),
            "map code lookup should resolve for parsed entry"
        );
        check!(
            dma.get_by_name(entry.name()).is_some(),
            "name lookup should resolve for parsed entry"
        );
    }

    for window in dma.entries().windows(2) {
        check!(
            window[0].offset() < window[1].offset(),
            "entry offsets should increase monotonically"
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
