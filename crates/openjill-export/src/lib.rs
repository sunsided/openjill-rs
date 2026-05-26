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
    use openjill_core::Palette;
    use openjill_core::entity::Rect;
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;
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

    /// Unit under test: [`dma::table_to_csv`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `DmaFile` reference and returning `String`.
    #[test]
    fn dma_csv_smoke_test_pins_signature() {
        let _: fn(&DmaFile) -> String = dma::table_to_csv;
    }

    /// Unit under test: [`dma::table_to_text`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `DmaFile` reference and returning `String`.
    #[test]
    fn dma_text_smoke_test_pins_signature() {
        let _: fn(&DmaFile) -> String = dma::table_to_text;
    }

    /// Unit under test: [`vcl::entries_to_text`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `VclFile` reference and returning `String`.
    #[test]
    fn vcl_text_smoke_test_pins_signature() {
        let _: fn(&VclFile) -> String = vcl::entries_to_text;
    }

    /// Unit under test: [`vcl::escape_text_payload`].
    ///
    /// Invariants asserted: the helper keeps accepting a text slice and
    /// returning an escaped `String`.
    #[test]
    fn vcl_escape_text_payload_smoke_test_pins_signature() {
        let _: fn(&str) -> String = vcl::escape_text_payload;
    }

    /// Unit under test: [`vcl::entries_to_json`].
    ///
    /// Invariants asserted: the export function keeps accepting a parsed
    /// `VclFile` reference and returning `String`.
    #[test]
    fn vcl_json_smoke_test_pins_signature() {
        let _: fn(&VclFile) -> String = vcl::entries_to_json;
    }

    /// Unit under test: [`cfg::file_to_rows`].
    ///
    /// Invariants asserted: the (not-yet-implemented) internal function keeps
    /// accepting a parsed `CfgFile` reference and returning `Vec<Row>`.
    #[test]
    fn cfg_smoke_test_pins_signature() {
        let _: fn(&CfgFile) -> Vec<super::Row> = cfg::file_to_rows;
    }

    /// Unit under test: [`cfg::scores_to_text`].
    ///
    /// Invariants asserted: the export function accepts a parsed `CfgFile`
    /// reference and returns `String`.
    #[test]
    fn cfg_scores_to_text_smoke_test_pins_signature() {
        let _: fn(&CfgFile) -> String = cfg::scores_to_text;
    }

    /// Unit under test: [`cfg::save_slots_to_text`].
    ///
    /// Invariants asserted: the export function accepts a parsed `CfgFile`
    /// reference plus a `jn_ext` slice and returns `String`.
    #[test]
    fn cfg_save_slots_to_text_smoke_test_pins_signature() {
        let _: fn(&CfgFile, &str) -> String = cfg::save_slots_to_text;
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
    /// Invariants asserted: the export function keeps accepting parsed JN/SHA/DMA
    /// references plus a palette and returning `RgbaImage`.
    #[test]
    fn jn_smoke_test_pins_signature() {
        let _: fn(&JnFile, &ShaFile, &DmaFile, &Palette) -> image::RgbaImage = jn::map_to_png;
    }

    /// Unit under test: [`jn::map_to_png_with_viewport`].
    ///
    /// Invariants asserted: the viewport overload accepts the same map inputs
    /// plus an optional clipping rectangle.
    #[test]
    fn jn_viewport_smoke_test_pins_signature() {
        let _: fn(&JnFile, &ShaFile, &DmaFile, &Palette, Option<Rect>) -> image::RgbaImage =
            jn::map_to_png_with_viewport;
    }
}
