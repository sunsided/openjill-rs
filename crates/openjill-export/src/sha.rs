//! SHA export stubs.

use image::RgbaImage;
use openjill_data::sha::ShaTileSet;

/// Selects how indexed SHA tiles are turned into export pixels.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TilesetColorOutput {
    /// Preserve the tileset's palette indices directly in PNG channel values.
    Indexed,
    /// Expand indices to RGB values using an explicit VGA palette.
    Colored {
        /// Explicit VGA palette entries used to resolve tile indices.
        palette: Box<[[u8; 3]; 256]>,
    },
}

/// Converts one parsed `*.SHA` tileset into an in-memory RGBA PNG image.
pub fn tileset_to_png(_tileset: &ShaTileSet, _output: TilesetColorOutput) -> RgbaImage {
    unimplemented!("SHA export wiring lands in a follow-up issue")
}
