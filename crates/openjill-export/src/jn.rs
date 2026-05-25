//! JN export stubs.

use image::RgbaImage;
use openjill_data::jn::JnFile;

/// Converts one parsed `*.JN1` map into an in-memory RGBA image.
///
/// Callers can encode the returned pixels as PNG if needed.
pub fn map_to_png(_file: &JnFile) -> RgbaImage {
    unimplemented!("JN export wiring lands in a follow-up issue")
}
