//! SHA export stubs.

use image::RgbaImage;
use openjill_data::sha::ShaTileSet;

/// Converts one parsed `*.SHA` tileset into an in-memory RGBA PNG image.
pub fn tileset_to_png(_tileset: &ShaTileSet) -> RgbaImage {
    unimplemented!("SHA export wiring lands in a follow-up issue")
}
