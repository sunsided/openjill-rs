//! JN export stubs.

use image::RgbaImage;
use openjill_data::jn::JnFile;

/// Converts one parsed `*.JN1` map into an in-memory RGBA PNG image.
pub fn map_to_png(_file: &JnFile) -> RgbaImage {
    unimplemented!("JN export wiring lands in a follow-up issue")
}
