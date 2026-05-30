//! Golden-output tests: pin the SHA-256 of each exporter's output against the
//! original episode 1 data, so an accidental change in any exporter (DMA, VCL,
//! CFG, SHA, JN) is caught as a byte-for-byte regression rather than a soft
//! structural drift.
//!
//! The text/CSV/JSON exporters are hashed over their UTF-8 bytes. The image
//! exporters are hashed over their *raw RGBA pixels* (`RgbaImage::as_raw`), not
//! a PNG encoding, so the goldens are independent of the PNG encoder version.
//!
//! The constants below were captured once from `data/original/JILL1` and are
//! the porting ground truth; a mismatch means an exporter changed its output.
//!
//! Self-skips when neither `OPENJILL_DATA_DIR` nor the default
//! `data/original/JILL1` path is present, so CI without the copyrighted bytes
//! still passes.

use openjill_core::Palette;
use openjill_data::DataDirectory;
use openjill_data::cfg::CfgFile;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_data::vcl::VclFile;
use openjill_export::{cfg, dma, jn, sha, vcl};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Environment variable that lets a developer override the data directory.
const DATA_DIR_ENV: &str = "OPENJILL_DATA_DIR";

/// Pinned SHA-256 of `dma::table_to_csv` over `JILL.DMA`.
const DMA_CSV: &str = "0ebfbee00cf5883fb47db490d914d4219e2fec16e95814a4d37c806ca4f9cc44";
/// Pinned SHA-256 of `dma::table_to_text` over `JILL.DMA`.
const DMA_TEXT: &str = "33be30a854c4e96f162c51f2116de976a64271fd880d4a2bf35178b5e05709e0";
/// Pinned SHA-256 of `vcl::entries_to_text` over `JILL1.VCL`.
const VCL_TEXT: &str = "813b69359583d1e0b88cafce84ef0b68be692f7f26a7529dc5c25eb23fffe9e9";
/// Pinned SHA-256 of `vcl::entries_to_json` over `JILL1.VCL`.
const VCL_JSON: &str = "9cb63783f8fef3ff123cc24628f5758b640e645e960b67d091148cbe80b0e1a3";
/// Pinned SHA-256 of `cfg::scores_to_text` over `JILL1.CFG`.
const CFG_SCORES: &str = "fc6599eed009d01c1875e0320d98e136eabcd015c2a226cbd1bf8a4e540e9259";
/// Pinned SHA-256 of `cfg::save_slots_to_text` over `JILL1.CFG`.
const CFG_SAVES: &str = "ed6a8ded98d5d4f619416f150e68629d2563025c47dd16ca36100893c615de6e";
/// Pinned SHA-256 of `sha::atlas_to_png` raw RGBA over `JILL1.SHA`.
const SHA_ATLAS: &str = "508a202653240d2c4655412f9fddcf6106a466b9872b93612c59b9f733ec9dc7";
/// Pinned SHA-256 of `jn::map_to_png` raw RGBA over `1.JN1`.
const JN_MAP: &str = "f5f70669df684c2f21a2949be40d07fd65c43a4685b962a3fd46e55aaa496bf9";

/// Resolves the data directory from `OPENJILL_DATA_DIR` or the workspace-relative
/// fallback path. Returns `None` when neither location is available.
fn resolve_data_dir(env_override: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    if let Some(path) = env_override {
        return Some(PathBuf::from(path));
    }
    let default = Path::new(env!("CARGO_WORKSPACE_DIR")).join("data/original/JILL1");
    Some(default).filter(|p| p.is_dir())
}

/// Reads `name` from `dir` case-insensitively and returns its raw bytes.
fn read_file(dir: &DataDirectory, name: &str) -> Vec<u8> {
    let path = dir
        .resolve_path_case_insensitive(name)
        .unwrap_or_else(|e| panic!("data file {name} must resolve: {e}"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("data file {name} must read: {e}"))
}

/// Lowercase hex SHA-256 of `bytes`.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Unit under test: every exporter's output over the original episode 1 data.
///
/// Preconditions: `OPENJILL_DATA_DIR` (or the default `data/original/JILL1`)
/// holds `JILL.DMA`, `JILL1.VCL`, `JILL1.CFG`, `JILL1.SHA`, and `1.JN1`. Skips
/// otherwise.
///
/// Invariants asserted: each exporter reproduces its pinned SHA-256 byte for
/// byte. Failure means an exporter changed its output - intentional changes must
/// re-capture the constant above.
#[test]
fn exporters_match_golden_outputs() {
    let env_override = std::env::var_os(DATA_DIR_ENV);
    let data_dir = match resolve_data_dir(env_override.as_deref()) {
        Some(dir) => dir,
        None => {
            eprintln!(
                "skipping golden-output test; {DATA_DIR_ENV} is not set \
                 and default data directory is missing"
            );
            return;
        }
    };
    let dir = DataDirectory::new(data_dir);

    let dma_file = DmaFile::from_bytes(read_file(&dir, "JILL.DMA")).expect("JILL.DMA must parse");
    let vcl_file = VclFile::from_bytes(read_file(&dir, "JILL1.VCL")).expect("JILL1.VCL must parse");
    let cfg_file =
        CfgFile::from_bytes(read_file(&dir, "JILL1.CFG"), "JN1").expect("JILL1.CFG must parse");
    let sha_file = ShaFile::from_bytes(read_file(&dir, "JILL1.SHA")).expect("JILL1.SHA must parse");
    let jn_file = JnFile::from_bytes(read_file(&dir, "1.JN1")).expect("1.JN1 must parse");
    let palette = Palette::jill_vga();

    assert_eq!(
        sha256_hex(dma::table_to_csv(&dma_file).as_bytes()),
        DMA_CSV,
        "DMA table_to_csv golden mismatch"
    );
    assert_eq!(
        sha256_hex(dma::table_to_text(&dma_file).as_bytes()),
        DMA_TEXT,
        "DMA table_to_text golden mismatch"
    );
    assert_eq!(
        sha256_hex(vcl::entries_to_text(&vcl_file).as_bytes()),
        VCL_TEXT,
        "VCL entries_to_text golden mismatch"
    );
    assert_eq!(
        sha256_hex(vcl::entries_to_json(&vcl_file).as_bytes()),
        VCL_JSON,
        "VCL entries_to_json golden mismatch"
    );
    assert_eq!(
        sha256_hex(cfg::scores_to_text(&cfg_file).as_bytes()),
        CFG_SCORES,
        "CFG scores_to_text golden mismatch"
    );
    assert_eq!(
        sha256_hex(cfg::save_slots_to_text(&cfg_file, "JN1").as_bytes()),
        CFG_SAVES,
        "CFG save_slots_to_text golden mismatch"
    );
    assert_eq!(
        sha256_hex(sha::atlas_to_png(&sha_file, &sha::AtlasOptions::default()).as_raw()),
        SHA_ATLAS,
        "SHA atlas_to_png golden mismatch"
    );
    assert_eq!(
        sha256_hex(jn::map_to_png(&jn_file, &sha_file, &dma_file, &palette).as_raw()),
        JN_MAP,
        "JN map_to_png golden mismatch"
    );
}
