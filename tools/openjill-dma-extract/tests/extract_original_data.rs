//! Integration test that runs the `openjill-dma-extract` binary against the
//! original `JILL.DMA` when the game data is available locally. Self-skips
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

/// Resolves `JILL.DMA`, returning `None` to self-skip when absent.
fn dma_path() -> Option<PathBuf> {
    let dma = resolve_data_dir()?.join("JILL.DMA");
    dma.is_file().then_some(dma)
}

#[test]
fn csv_dump_has_header_and_round_trippable_rows() {
    let Some(dma) = dma_path() else {
        eprintln!("skipping: JILL.DMA not found");
        return;
    };

    let output = Command::new(env!("CARGO_BIN_EXE_openjill-dma-extract"))
        .arg("--file")
        .arg(&dma)
        .arg("--csv")
        .output()
        .expect("dma-extract binary should run");
    assert!(output.status.success(), "csv dump should exit successfully");
    let text = String::from_utf8_lossy(&output.stdout);

    let mut lines = text.lines();
    assert_eq!(
        lines.next(),
        Some("map_code,tileset,tile,flags,flag_names"),
        "first line should be the CSV header"
    );

    let mut count = 0;
    for line in lines.filter(|l| !l.is_empty()) {
        let fields: Vec<&str> = line.split(',').collect();
        assert!(
            fields.len() == 5,
            "each row should have five columns: {line}"
        );
        // The first four columns are numeric (round-trippable).
        fields[0].parse::<u16>().expect("map_code is numeric");
        fields[1].parse::<u8>().expect("tileset is numeric");
        fields[2].parse::<u8>().expect("tile is numeric");
        fields[3].parse::<u16>().expect("flags is numeric");
        count += 1;
    }
    assert!(count > 0, "DMA should contain at least one entry");
}

#[test]
fn json_dump_parses_as_array() {
    let Some(dma) = dma_path() else {
        eprintln!("skipping: JILL.DMA not found");
        return;
    };

    let output = Command::new(env!("CARGO_BIN_EXE_openjill-dma-extract"))
        .arg("--file")
        .arg(&dma)
        .arg("--json")
        .output()
        .expect("dma-extract binary should run");
    assert!(
        output.status.success(),
        "json dump should exit successfully"
    );

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("output should be valid JSON");
    let array = value.as_array().expect("top-level JSON should be an array");
    assert!(!array.is_empty(), "JSON array should contain entries");
    let first = &array[0];
    for key in ["map_code", "tileset", "tile", "flags"] {
        assert!(first.get(key).is_some(), "entry should have a {key} field");
    }
}
