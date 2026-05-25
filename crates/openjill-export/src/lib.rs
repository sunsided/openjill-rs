//! Export-layer crate for OpenJill parser data.
//!
//! This crate defines the public export-module split and return types used by
//! future format-specific export implementations.

#![forbid(unsafe_code)]

/// Export helpers for `JILL1.CFG` high-score, save-slot, and setup data.
pub mod cfg;
/// Export helpers for `JILL.DMA` tile-metadata data.
pub mod dma;
/// Export helpers for `*.JN1` map and level data.
pub mod jn;
/// Export helpers for `*.SHA` tileset and image data.
pub mod sha;
/// Export helpers for `*.VCL` text-entry data.
pub mod vcl;

/// One exported table row for non-image formats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Row {
    /// Human-readable row key.
    pub key: String,
    /// Human-readable row value.
    pub value: String,
}

/// Smoke tests that pin the requested public API to synthetic parser fixtures.
#[cfg(test)]
mod tests {
    use super::{cfg, dma, jn, sha, vcl};
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;
    use openjill_data::vcl::VclFile;
    use std::panic::{self, AssertUnwindSafe};

    /// Runs one export stub and asserts that the current placeholder body panics.
    fn assert_stub_panics<R>(f: impl FnOnce() -> R) {
        assert!(
            panic::catch_unwind(AssertUnwindSafe(f)).is_err(),
            "stub should still panic with unimplemented!()"
        );
    }

    /// Unit under test: [`dma::file_to_rows`].
    ///
    /// Preconditions: a valid synthetic `JILL.DMA` buffer with one parsed entry.
    ///
    /// Invariants asserted: the export function accepts the parsed `DmaFile`
    /// value and currently remains an `unimplemented!()` stub.
    #[test]
    fn dma_smoke_test_builds_against_synthetic_fixture() {
        let file = DmaFile::from_bytes(valid_dma_bytes()).expect("parse synthetic DMA fixture");
        assert_stub_panics(|| dma::file_to_rows(&file));
    }

    /// Unit under test: [`vcl::file_to_string`].
    ///
    /// Preconditions: a valid synthetic `JILL1.VCL` buffer with one non-empty
    /// parsed text entry.
    ///
    /// Invariants asserted: the export function accepts the parsed `VclFile`
    /// value and currently remains an `unimplemented!()` stub.
    #[test]
    fn vcl_smoke_test_builds_against_synthetic_fixture() {
        let file = VclFile::from_bytes(valid_vcl_bytes()).expect("parse synthetic VCL fixture");
        assert_stub_panics(|| vcl::file_to_string(&file));
    }

    /// Unit under test: [`cfg::file_to_rows`].
    ///
    /// Preconditions: a valid synthetic `JILL1.CFG` buffer and the `JN1`
    /// save-file prefix expected by the parser.
    ///
    /// Invariants asserted: the export function accepts the parsed `CfgFile`
    /// value and currently remains an `unimplemented!()` stub.
    #[test]
    fn cfg_smoke_test_builds_against_synthetic_fixture() {
        let file =
            CfgFile::from_bytes(valid_cfg_bytes(), "JN1").expect("parse synthetic CFG fixture");
        assert_stub_panics(|| cfg::file_to_rows(&file));
    }

    /// Unit under test: [`sha::tileset_to_png`].
    ///
    /// Preconditions: a valid synthetic `JILL1.SHA` buffer with one concrete
    /// parsed tileset.
    ///
    /// Invariants asserted: the export function accepts the parsed
    /// `ShaTileSet` value and both indexed and explicit-palette output modes,
    /// while currently remaining an `unimplemented!()` stub.
    #[test]
    fn sha_smoke_test_builds_against_synthetic_fixture() {
        let file = ShaFile::from_bytes(valid_sha_bytes()).expect("parse synthetic SHA fixture");
        let tileset = file
            .tilesets()
            .first()
            .expect("synthetic SHA fixture should contain one tileset");
        assert_stub_panics(|| sha::tileset_to_png(tileset, sha::TilesetColorOutput::Indexed));
        assert_stub_panics(|| {
            sha::tileset_to_png(
                tileset,
                sha::TilesetColorOutput::Colored {
                    palette: Box::new([[0u8; 3]; 256]),
                },
            )
        });
    }

    /// Unit under test: [`jn::map_to_png`].
    ///
    /// Preconditions: a valid synthetic `*.JN1` buffer with empty object and
    /// string sections.
    ///
    /// Invariants asserted: the export function accepts the parsed `JnFile`
    /// value and currently remains an `unimplemented!()` stub.
    #[test]
    fn jn_smoke_test_builds_against_synthetic_fixture() {
        let file = JnFile::from_bytes(valid_jn_bytes()).expect("parse synthetic JN fixture");
        assert_stub_panics(|| jn::map_to_png(&file));
    }

    /// Builds one valid synthetic `JILL.DMA` file with a single entry.
    fn valid_dma_bytes() -> Vec<u8> {
        vec![
            0x01, 0x00, // map_code
            0x02, // tile
            0x03, // tileset
            0x00, 0x00, // flags
            0x01, // name_len
            b'A', // name
        ]
    }

    /// Builds one valid synthetic `JILL1.VCL` file with one non-empty text entry.
    fn valid_vcl_bytes() -> Vec<u8> {
        let mut bytes = vec![0u8; 701];
        bytes[400..404].copy_from_slice(&(700u32).to_le_bytes());
        bytes[560..562].copy_from_slice(&(1u16).to_le_bytes());
        bytes[700] = b'A';
        bytes
    }

    /// Builds one valid synthetic `JILL1.CFG` file with defaulted values.
    fn valid_cfg_bytes() -> Vec<u8> {
        vec![0u8; 254]
    }

    /// Builds one valid synthetic `*.JN1` file with empty object and string sections.
    fn valid_jn_bytes() -> Vec<u8> {
        vec![0u8; 16_456]
    }

    /// Builds one valid synthetic `JILL1.SHA` file with one concrete tileset.
    fn valid_sha_bytes() -> Vec<u8> {
        const HEADER_ENTRY_COUNT: usize = 128;
        const HEADER_LEN: usize = (HEADER_ENTRY_COUNT * 4) + (HEADER_ENTRY_COUNT * 2);

        let tileset_offset = HEADER_LEN;
        let tileset = {
            let mut bytes = Vec::new();
            bytes.push(1);
            bytes.extend(1u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.extend(3u16.to_le_bytes());
            bytes.push(8);
            bytes.extend(0x0001u16.to_le_bytes());
            bytes.extend([
                3u8, 3u8, 0u8, 7u8, 8u8, 9u8, 10u8, 11u8, 12u8, 13u8, 14u8, 15u8,
            ]);
            bytes
        };

        let mut bytes = vec![0u8; HEADER_LEN];
        bytes.extend_from_slice(&tileset);

        let offset_bytes = u32::try_from(tileset_offset)
            .expect("tileset offset must fit u32")
            .to_le_bytes();
        bytes[..4].copy_from_slice(&offset_bytes);

        let size_bytes = u16::try_from(tileset.len())
            .expect("tileset size must fit u16")
            .to_le_bytes();
        bytes[(HEADER_ENTRY_COUNT * 4)..(HEADER_ENTRY_COUNT * 4 + 2)].copy_from_slice(&size_bytes);

        bytes
    }
}
