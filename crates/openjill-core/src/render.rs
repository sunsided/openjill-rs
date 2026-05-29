//! Rendering instruction type produced by screen handlers each tick.

/// Axis-aligned framebuffer rectangle in pixel coordinates.
///
/// Used to clip individual `RenderCommand` outputs to a sub-region of the
/// 320x200 framebuffer, e.g. so background tiles emitted by a level screen
/// do not bleed past the right edge of the game area into the status bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipRect {
    /// Left edge of the clip rectangle in framebuffer pixels.
    pub x: i32,
    /// Top edge of the clip rectangle in framebuffer pixels.
    pub y: i32,
    /// Width of the clip rectangle in pixels.
    pub width: u32,
    /// Height of the clip rectangle in pixels.
    pub height: u32,
}

impl ClipRect {
    /// Returns `true` when `(px, py)` lies inside the clip rectangle.
    ///
    /// `(px, py)` is in framebuffer pixel coordinates.  An empty rectangle
    /// (`width == 0` or `height == 0`) contains no points.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        // Use `saturating_add_unsigned` so a `u32` width or height larger
        // than `i32::MAX` is clamped to `i32::MAX` instead of wrapping into
        // a negative value via an `as i32` cast.  Negative bounds would
        // make a non-empty rectangle report "contains nothing".
        let right = self.x.saturating_add_unsigned(self.width);
        let bottom = self.y.saturating_add_unsigned(self.height);
        px >= self.x && px < right && py >= self.y && py < bottom
    }
}

/// Per-command font selector used by [`RenderCommand::DrawText`].
///
/// The shipped `JILL1.SHA` carries several font tilesets that differ in
/// glyph size; the original DOS renderer picks one of them per text label
/// it draws.  The Rust port mirrors that distinction so identical labels
/// land at identical visual sizes:
///
/// - [`FontSize::Small`] (6x6 px) – the default body text used by status
///   bar labels, message-bar status text, the level-transition message
///   box, and most in-game DrawText paths.
/// - [`FontSize::Big`] (8x8 px) – the larger "title" font used by
///   `status_bar_vga.json`'s `bigtext` array and similar label slots
///   that the JSON tags as `bigtext`.
///
/// New variants can be added later for the digit-only (`4x5`) and huge
/// menu (`32x8`) tilesets once a draw path actually needs them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FontSize {
    /// 6x6 px body text font (typically the second font tileset).
    Small,
    /// 8x8 px title / `bigtext` font (typically the first font tileset).
    Big,
}

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
    /// When `clip` is `Some`, the renderer only writes pixels whose
    /// framebuffer coordinates lie inside the clip rectangle; otherwise the
    /// blit is clipped only to the 320x200 framebuffer bounds.
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
        /// Optional per-command clip rectangle in framebuffer pixels.
        clip: Option<ClipRect>,
    },
    /// Draw a text string at `(x, y)` using a SHA font tileset.
    DrawText {
        /// Text to render.
        text: String,
        /// Destination x coordinate in framebuffer pixels.
        x: i32,
        /// Destination y coordinate in framebuffer pixels.
        y: i32,
        /// Logical EGA color index `0..=7` used to colorize each text glyph.
        ///
        /// The renderer adds `TEXT_COLOR_BRIGHT_SHIFT` (8) and clamps the
        /// result to `15` before writing pixels, mirroring
        /// `TextManager.initColorTextMap` in the Java reference: callers pass
        /// the dark-EGA index and the text path always uses the matching
        /// bright-EGA color (index `i + 8`).  Values above 7 are saturated to
        /// 15 (bright white).
        color_index: u8,
        /// Which SHA font tileset to draw with.  See [`FontSize`] for the
        /// per-variant mapping; emitters that do not have a specific
        /// preference should pass [`FontSize::Small`], which matches the
        /// default body-text font the original DOS game renders with.
        font: FontSize,
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
    /// Draw one glyph from a SHA *font* tileset, recolored and with its
    /// background keyed out.
    ///
    /// Unlike [`RenderCommand::Blit`] (which writes a tile's raw indexed
    /// pixels), this treats `tileset` as a font: the glyph at `tile` is filled
    /// with `color_index` (promoted to bright EGA like [`RenderCommand::DrawText`])
    /// and the font's transparent background pixels are skipped.  Used for the
    /// control-panel key-cap glyphs (SHIFT / ALT / F1) which live in a font
    /// tileset and are recolored via `grapSpecialKey` in the Java reference.
    BlitGlyph {
        /// SHA font tileset index (0-based).
        tileset: u8,
        /// Glyph (tile) index within the font tileset.
        tile: u16,
        /// Destination x coordinate in framebuffer pixels.
        x: i32,
        /// Destination y coordinate in framebuffer pixels.
        y: i32,
        /// Logical EGA color index `0..=7`; the renderer applies the same
        /// bright-EGA shift as [`RenderCommand::DrawText`].
        color_index: u8,
    },
}

#[cfg(test)]
mod tests {
    use super::{ClipRect, FontSize, RenderCommand};

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
            clip: None,
        };
        assert_eq!(blit.clone(), blit);

        let draw_text = RenderCommand::DrawText {
            text: String::from("Hello"),
            x: 10,
            y: 20,
            color_index: 15,
            font: FontSize::Small,
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
            clip: Some(ClipRect {
                x: 80,
                y: 16,
                width: 232,
                height: 160,
            }),
        };
        if let RenderCommand::Blit {
            tileset,
            tile,
            x,
            y,
            opaque,
            clip,
        } = cmd
        {
            assert_eq!(tileset, 7);
            assert_eq!(tile, 1023);
            assert_eq!(x, -10);
            assert_eq!(y, 200);
            assert!(opaque);
            assert_eq!(
                clip,
                Some(ClipRect {
                    x: 80,
                    y: 16,
                    width: 232,
                    height: 160,
                })
            );
        } else {
            panic!("expected Blit variant");
        }
    }

    /// Unit under test: `ClipRect::contains` clamps oversized dimensions
    /// instead of wrapping when `width` or `height` exceeds `i32::MAX`.
    ///
    /// Preconditions: a clip rectangle with `width = u32::MAX`.  Casting that
    /// width to `i32` via `as` would wrap to `-1` and make every point
    /// "outside" the rectangle.
    ///
    /// Invariants asserted: a point comfortably inside the rectangle still
    /// reports as contained, confirming the saturating addition path.
    #[test]
    fn clip_rect_contains_saturates_oversized_dimensions() {
        let rect = ClipRect {
            x: 0,
            y: 0,
            width: u32::MAX,
            height: u32::MAX,
        };
        assert!(rect.contains(100, 100));
        assert!(rect.contains(i32::MAX - 1, i32::MAX - 1));
    }

    /// Unit under test: `ClipRect::contains`.
    ///
    /// Preconditions: a clip rectangle with positive width and height.
    ///
    /// Invariants asserted: the left/top edges are inclusive, the right/bottom
    /// edges are exclusive, and empty rectangles contain no points.
    #[test]
    fn clip_rect_contains_respects_half_open_bounds() {
        let rect = ClipRect {
            x: 80,
            y: 16,
            width: 232,
            height: 160,
        };
        assert!(rect.contains(80, 16));
        assert!(rect.contains(311, 175));
        assert!(!rect.contains(312, 16));
        assert!(!rect.contains(80, 176));
        assert!(!rect.contains(79, 16));

        let empty = ClipRect {
            x: 0,
            y: 0,
            width: 0,
            height: 10,
        };
        assert!(!empty.contains(0, 0));
    }
}
