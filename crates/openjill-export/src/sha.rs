//! SHA export stubs.

use image::RgbaImage;
use openjill_data::sha::ShaTileSet;
use std::sync::Arc;

/// Selects how indexed SHA tiles are turned into export pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TilesetColorOutput {
    /// Preserve palette indices directly in RGBA channels.
    ///
    /// Exporters should write each pixel index `i` as `R=i, G=i, B=i, A=255`.
    /// This keeps indexed semantics intact for tools that post-process indices.
    Indexed,
    /// Expand indices to RGB values using an explicit VGA palette.
    ///
    /// Exporters should resolve each index `i` through `palette[i]` and emit
    /// `R, G, B` from that triplet with `A=255`.
    Colored {
        /// Explicit VGA palette entries used to resolve tile indices.
        palette: Arc<[[u8; 3]; 256]>,
    },
}

/// Converts one parsed `*.SHA` tileset into an in-memory RGBA PNG image.
///
/// The `output` mode controls whether export preserves raw indices
/// ([`TilesetColorOutput::Indexed`]) or resolves them using an explicit palette
/// ([`TilesetColorOutput::Colored`]).
pub fn tileset_to_png(_tileset: &ShaTileSet, _output: TilesetColorOutput) -> RgbaImage {
    unimplemented!("SHA export wiring lands in a follow-up issue")
}
