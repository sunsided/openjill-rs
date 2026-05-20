#![forbid(unsafe_code)]

use openjill_data::DataDirectory;
use openjill_data::sha::ShaColorMapEntry;

#[derive(Clone, Debug)]
pub struct CoreState {
    data_directory: DataDirectory,
}

impl CoreState {
    pub fn new(data_directory: DataDirectory) -> Self {
        Self { data_directory }
    }

    pub fn data_directory(&self) -> &DataDirectory {
        &self.data_directory
    }
}

/// One expanded 256-entry VGA palette used for indexed-framebuffer presentation.
#[derive(Clone, Debug)]
pub struct Palette {
    /// Expanded 8-bit RGB entries indexed by framebuffer color index.
    entries: [[u8; 3]; 256],
}

impl Palette {
    /// Builds a palette from a fully expanded 256-entry RGB table.
    pub fn new(entries: [[u8; 3]; 256]) -> Self {
        Self { entries }
    }

    /// Returns the expanded 8-bit RGB entries indexed by framebuffer color index.
    pub fn entries(&self) -> &[[u8; 3]; 256] {
        &self.entries
    }

    /// Builds a palette from SHA color-map entries.
    ///
    /// The SHA on-disk color map stores one 4-byte entry per indexed color. Each entry
    /// contains three bytes for the three video modes the engine supported, followed by a
    /// reserved byte. In VGA mode the three meaningful bytes are the R, G, B components
    /// (fields named `cga`, `ega`, `vga` respectively by the original data format convention):
    ///
    /// | on-disk field | video-mode name | VGA meaning |
    /// |---------------|----------------|-------------|
    /// | `cga`         | CGA index       | Red         |
    /// | `ega`         | EGA index       | Green       |
    /// | `vga`         | VGA index       | Blue        |
    ///
    /// Each component is a 6-bit value in the range `0..=63`. It is expanded to 8-bit with
    /// `(value << 2) | (value >> 4)`, which preserves both boundary values exactly
    /// (`0 → 0`, `63 → 255`) while spreading intermediate values uniformly.
    pub fn from_sha_color_map(entries: &[ShaColorMapEntry]) -> Self {
        let mut expanded = [[0_u8; 3]; 256];
        for (destination, entry) in expanded.iter_mut().zip(entries.iter()) {
            *destination = [
                expand_6bit_component(entry.cga()),
                expand_6bit_component(entry.ega()),
                expand_6bit_component(entry.vga()),
            ];
        }
        Self { entries: expanded }
    }

    /// Builds a synthetic greyscale palette used when no SHA color map is available.
    pub fn greyscale_fallback() -> Self {
        let mut entries = [[0_u8; 3]; 256];
        for (index, color) in entries.iter_mut().enumerate() {
            let value = index as u8;
            *color = [value, value, value];
        }
        Self { entries }
    }

    /// Returns one palette entry as opaque RGBA bytes.
    pub fn rgba(&self, index: u8) -> [u8; 4] {
        let [r, g, b] = self.entries[index as usize];
        [r, g, b, 255]
    }
}

impl Default for Palette {
    /// Returns the greyscale fallback palette as the startup default.
    fn default() -> Self {
        Self::greyscale_fallback()
    }
}

/// Expands one 6-bit VGA component to 8-bit color space.
///
/// Masks to six bits and then combines a left-shift and a top-bit fill so that
/// 0 → 0 and 63 → 255 are preserved exactly.
fn expand_6bit_component(value: u8) -> u8 {
    let value = value & 0x3f;
    (value << 2) | (value >> 4)
}

#[cfg(test)]
mod tests {
    use super::Palette;
    use openjill_data::sha::ShaColorMapEntry;

    /// Unit under test: `Palette::from_sha_color_map`.
    ///
    /// Preconditions: a synthetic SHA color-map fixture sets explicit 6-bit boundary component
    /// values (`0` and `63`) and the first palette index follows the transparent-color convention
    /// (`0,0,0`).
    ///
    /// Invariants asserted: 6-bit components expand to the expected 8-bit boundaries (`0` and
    /// `255`), index 0 stays black consistent with the transparent convention, and `rgba` always
    /// returns opaque alpha 255.
    #[test]
    fn from_sha_color_map_expands_boundaries_and_keeps_alpha_opaque() {
        let entries = [
            ShaColorMapEntry::new(0, 0, 0, 0),
            ShaColorMapEntry::new(63, 63, 63, 0),
        ];
        let palette = Palette::from_sha_color_map(&entries);
        assert_eq!(palette.rgba(0), [0, 0, 0, 255]);
        assert_eq!(palette.rgba(1), [255, 255, 255, 255]);
    }

    /// Unit under test: `Palette::greyscale_fallback`.
    ///
    /// Preconditions: no SHA color-map entries are provided; the fallback constructor is called
    /// directly.
    ///
    /// Invariants asserted: index 0 is black for transparency semantics, intermediate indices
    /// map to deterministic greyscale values, index 255 is white, and every entry is opaque.
    #[test]
    fn greyscale_fallback_is_deterministic_and_opaque() {
        let palette = Palette::greyscale_fallback();
        assert_eq!(palette.rgba(0), [0, 0, 0, 255]);
        assert_eq!(palette.rgba(127), [127, 127, 127, 255]);
        assert_eq!(palette.rgba(255), [255, 255, 255, 255]);
    }
}
