//! Integration test that runs the `openjill-jn-extract` binary against the
//! original `1.JN1` map when the game data is available locally. Self-skips
//! when the data directory is missing so CI passes without copyrighted bytes.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Environment variable overriding the data directory at runtime.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Full rendered JN map dimensions (`128 * 16` by `64 * 16`).
const MAP_WIDTH: usize = 2048;
const MAP_HEIGHT: usize = 1024;

/// Resolves the data directory, preferring `OPENJILL_DATA_DIR` and falling
/// back to the workspace `data/original/JILL1`. Returns `None` to self-skip.
fn resolve_data_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(DATA_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    default.is_dir().then_some(default)
}

/// Reads a PNG's `IHDR` width/height (big-endian u32 at bytes 16..24).
fn png_dimensions(bytes: &[u8]) -> Option<(usize, usize)> {
    let signature = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[..8] != signature {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?) as usize;
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?) as usize;
    Some((width, height))
}

#[test]
fn renders_full_map_png_from_original_jn() {
    let Some(data_dir) = resolve_data_dir() else {
        eprintln!("skipping: {DATA_DIR_ENV} unset and default data directory missing");
        return;
    };
    let jn = data_dir.join("1.JN1");
    if !jn.is_file() {
        eprintln!("skipping: {} not found", jn.display());
        return;
    }

    let out = std::env::temp_dir().join(format!("openjill-jn-extract-{}.png", std::process::id()));
    let _ = std::fs::remove_file(&out);

    let status = Command::new(env!("CARGO_BIN_EXE_openjill-jn-extract"))
        .arg("--file")
        .arg(&jn)
        .arg("--out")
        .arg(&out)
        .status()
        .expect("jn-extract binary should run");
    assert!(status.success(), "extract should exit successfully");

    let bytes = std::fs::read(&out).expect("output PNG should exist");
    let (width, height) = png_dimensions(&bytes).expect("output should be a valid PNG");
    assert_eq!(
        (width, height),
        (MAP_WIDTH, MAP_HEIGHT),
        "full map dimensions"
    );

    let _ = std::fs::remove_file(&out);
}

#[test]
fn objects_dump_lists_object_layer() {
    let Some(data_dir) = resolve_data_dir() else {
        eprintln!("skipping: {DATA_DIR_ENV} unset and default data directory missing");
        return;
    };
    let jn = data_dir.join("1.JN1");
    if !jn.is_file() {
        eprintln!("skipping: {} not found", jn.display());
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_openjill-jn-extract"))
        .arg("--file")
        .arg(&jn)
        .arg("--objects")
        .output()
        .expect("jn-extract binary should run");
    assert!(
        output.status.success(),
        "objects dump should exit successfully"
    );
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("Object layer"),
        "dump should list the object layer"
    );
    assert!(
        text.contains("Save data:"),
        "dump should include the save-data summary"
    );
}
