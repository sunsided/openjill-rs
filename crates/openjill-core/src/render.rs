//! Rendering instruction type produced by screen handlers each tick.

/// One rendering instruction produced by a screen handler per tick.
///
/// The renderer executes these in order against its indexed framebuffer.
/// No GPU or windowing imports appear in this type; the render crate owns
/// the actual execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenderCommand {
    /// Fill the entire framebuffer with palette index `color`.
    Clear {
        /// Palette index used to fill every pixel.
        color: u8,
    },
    /// Blit one SHA tile at framebuffer position `(x, y)`.
    ///
    /// Pixel index 0 in the tile is treated as transparent unless `opaque` is true.
    Blit {
        /// SHA tileset index (0-based).
        tileset: u8,
        /// Tile index within the tileset.
        tile: u16,
        /// Destination x coordinate in framebuffer pixels.
        x: i32,
        /// Destination y coordinate in framebuffer pixels.
        y: i32,
        /// When true, pixel index 0 is rendered as an opaque color rather than skipped.
        opaque: bool,
    },
    /// Draw a text string at `(x, y)` using the SHA font tileset.
    DrawText {
        /// Text to render.
        text: String,
        /// Destination x coordinate in framebuffer pixels.
        x: i32,
        /// Destination y coordinate in framebuffer pixels.
        y: i32,
        /// Palette index used to colorize each text glyph.
        color_index: u8,
    },
    /// Fill a rectangle with palette index `color`.
    FillRect {
        /// Left edge of the filled region in framebuffer pixels.
        x: i32,
        /// Top edge of the filled region in framebuffer pixels.
        y: i32,
        /// Width of the filled region in pixels.
        width: u32,
        /// Height of the filled region in pixels.
        height: u32,
        /// Palette index used to fill every pixel in the region.
        color: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::RenderCommand;

    /// Unit under test: all `RenderCommand` variants.
    ///
    /// Preconditions: none; this test only validates that each variant
    /// can be constructed with representative field values and round-trips
    /// through `Clone` and `PartialEq` correctly.
    ///
    /// Invariants asserted: each variant constructs without error and
    /// compares equal to an identical copy.
    #[test]
    fn render_command_variants_construct_and_clone() {
        let clear = RenderCommand::Clear { color: 0 };
        assert_eq!(clear.clone(), clear);

        let blit = RenderCommand::Blit {
            tileset: 3,
            tile: 42,
            x: 80,
            y: 16,
            opaque: false,
        };
        assert_eq!(blit.clone(), blit);

        let draw_text = RenderCommand::DrawText {
            text: String::from("Hello"),
            x: 10,
            y: 20,
            color_index: 15,
        };
        assert_eq!(draw_text.clone(), draw_text);

        let fill_rect = RenderCommand::FillRect {
            x: 0,
            y: 0,
            width: 320,
            height: 200,
            color: 1,
        };
        assert_eq!(fill_rect.clone(), fill_rect);
    }

    /// Unit under test: `RenderCommand` field round-trip.
    ///
    /// Preconditions: a `Blit` command is constructed with distinct field values.
    ///
    /// Invariants asserted: field values survive construction and are accessible
    /// via pattern matching with the expected types.
    #[test]
    fn blit_fields_are_accessible() {
        let cmd = RenderCommand::Blit {
            tileset: 7,
            tile: 1023,
            x: -10,
            y: 200,
            opaque: true,
        };
        if let RenderCommand::Blit {
            tileset,
            tile,
            x,
            y,
            opaque,
        } = cmd
        {
            assert_eq!(tileset, 7);
            assert_eq!(tile, 1023);
            assert_eq!(x, -10);
            assert_eq!(y, 200);
            assert!(opaque);
        } else {
            panic!("expected Blit variant");
        }
    }
}
