//! Integration test that runs the `openjill-sha-extract` binary against the
//! original `JILL1.SHA` when the game data is available locally. Self-skips
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

/// Unique scratch directory under the system temp dir for this test process.
fn scratch_dir(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "openjill-sha-extract-{}-{}",
        tag,
        std::process::id()
    ))
}

#[test]
fn extracts_tilesets_to_png_from_original_sha() {
    let Some(data_dir) = resolve_data_dir() else {
        eprintln!("skipping: {DATA_DIR_ENV} unset and default data directory missing");
        return;
    };
    let sha = data_dir.join("JILL1.SHA");
    if !sha.is_file() {
        eprintln!("skipping: {} not found", sha.display());
        return;
    }

    let out = scratch_dir("extract");
    let _ = std::fs::remove_dir_all(&out);

    let status = Command::new(env!("CARGO_BIN_EXE_openjill-sha-extract"))
        .arg("--file")
        .arg(&sha)
        .arg("--out")
        .arg(&out)
        .status()
        .expect("sha-extract binary should run");
    assert!(status.success(), "extract should exit successfully");

    let pngs: Vec<_> = std::fs::read_dir(&out)
        .expect("output directory should exist")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "png"))
        .collect();
    assert!(
        !pngs.is_empty(),
        "at least one tileset PNG should be written"
    );

    // Every output must be a real PNG (8-byte signature).
    let signature = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    for png in &pngs {
        let bytes = std::fs::read(png.path()).expect("PNG should be readable");
        assert!(
            bytes.len() > 8 && bytes[..8] == signature,
            "{} should start with the PNG signature",
            png.path().display()
        );
    }

    let _ = std::fs::remove_dir_all(&out);
}

#[test]
fn dump_prints_header_and_tileset_tables() {
    let Some(data_dir) = resolve_data_dir() else {
        eprintln!("skipping: {DATA_DIR_ENV} unset and default data directory missing");
        return;
    };
    let sha = data_dir.join("JILL1.SHA");
    if !sha.is_file() {
        eprintln!("skipping: {} not found", sha.display());
        return;
    }

    let output = Command::new(env!("CARGO_BIN_EXE_openjill-sha-extract"))
        .arg("--file")
        .arg(&sha)
        .arg("--dump")
        .output()
        .expect("sha-extract binary should run");
    assert!(output.status.success(), "dump should exit successfully");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("Header entries:"),
        "dump should list header entries"
    );
    assert!(text.contains("Tilesets:"), "dump should list tilesets");
}
