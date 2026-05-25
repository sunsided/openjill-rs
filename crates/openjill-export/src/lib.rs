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

/// Smoke tests that pin the requested public API signatures.
#[cfg(test)]
mod tests {
    use super::{cfg, dma, jn, sha, vcl};
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaTileSet;
    use openjill_data::vcl::VclFile;

    /// Unit under test: [`dma::file_to_rows`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `DmaFile` reference and returning `Vec<Row>`.
    #[test]
    fn dma_smoke_test_pins_signature() {
        let _: fn(&DmaFile) -> Vec<super::Row> = dma::file_to_rows;
    }

    /// Unit under test: [`vcl::file_to_string`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `VclFile` reference and returning `String`.
    #[test]
    fn vcl_smoke_test_pins_signature() {
        let _: fn(&VclFile) -> String = vcl::file_to_string;
    }

    /// Unit under test: [`cfg::file_to_rows`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `CfgFile` reference and returning `Vec<Row>`.
    #[test]
    fn cfg_smoke_test_pins_signature() {
        let _: fn(&CfgFile) -> Vec<super::Row> = cfg::file_to_rows;
    }

    /// Unit under test: [`sha::tileset_to_png`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `ShaTileSet` reference plus a color-output mode and returns `RgbaImage`.
    #[test]
    fn sha_smoke_test_pins_signature() {
        let _: fn(&ShaTileSet, sha::TilesetColorOutput) -> image::RgbaImage = sha::tileset_to_png;
    }

    /// Unit under test: [`jn::map_to_png`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `JnFile` reference and returning `RgbaImage`.
    #[test]
    fn jn_smoke_test_pins_signature() {
        let _: fn(&JnFile) -> image::RgbaImage = jn::map_to_png;
    }
}
