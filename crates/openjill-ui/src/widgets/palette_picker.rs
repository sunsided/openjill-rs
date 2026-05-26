use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

/// Palette slices exposed by [`PalettePicker`] for the Jill VGA palette layout.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub enum PaletteFilter {
    /// Show all 256 colors.
    #[default]
    All,
    /// Show EGA base colors (`0..16`).
    Ega,
    /// Show greyscale ramp (`16..24`).
    Greyscale,
    /// Show VGA color cube (`24..256`).
    Vga,
}

impl PaletteFilter {
    /// All filter values in UI order.
    pub const ALL: [Self; 4] = [Self::All, Self::Ega, Self::Greyscale, Self::Vga];

    /// Human-readable filter label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Ega => "EGA",
            Self::Greyscale => "Greyscale",
            Self::Vga => "VGA",
        }
    }

    fn index_range(self) -> std::ops::Range<usize> {
        match self {
            Self::All => 0..256,
            Self::Ega => 0..16,
            Self::Greyscale => 16..24,
            Self::Vga => 24..256,
        }
    }
}

/// Output from [`PalettePicker::show`].
pub struct PalettePickerOutput {
    /// Combined egui response produced from all swatches in this picker.
    pub response: Response,
    /// Palette index clicked this frame.
    pub clicked_index: Option<u8>,
}

/// Egui widget that renders a clickable swatch grid for a 256-entry RGB palette.
pub struct PalettePicker<'a> {
    entries: &'a [[u8; 3]; 256],
    selected: &'a mut Option<u8>,
    swatch_size: f32,
    filter: PaletteFilter,
}

impl<'a> PalettePicker<'a> {
    /// Creates a palette picker for a 256-entry RGB palette and mutable selection.
    pub fn new(entries: &'a [[u8; 3]; 256], selected: &'a mut Option<u8>) -> Self {
        Self {
            entries,
            selected,
            swatch_size: 16.0,
            filter: PaletteFilter::All,
        }
    }

    /// Sets one swatch edge size in points.
    pub fn swatch_size(mut self, swatch_size: f32) -> Self {
        self.swatch_size = swatch_size.max(4.0);
        self
    }

    /// Restricts visible entries to one palette slice.
    pub fn filter(mut self, filter: PaletteFilter) -> Self {
        self.filter = filter;
        self
    }

    /// Adds the widget to a UI and returns response plus clicked palette index.
    pub fn show(self, ui: &mut Ui) -> PalettePickerOutput {
        const COLUMNS: usize = 16;

        let visible = self.filter.index_range();
        let count = visible.len();
        let rows = count.div_ceil(COLUMNS);
        let swatch_size = Vec2::splat(self.swatch_size);
        let grid_size = Vec2::new(COLUMNS as f32 * swatch_size.x, rows as f32 * swatch_size.y);
        let (grid_rect, mut response) = ui.allocate_exact_size(grid_size, Sense::click());
        if !ui.is_rect_visible(grid_rect) {
            return PalettePickerOutput {
                response,
                clicked_index: None,
            };
        }

        let mut clicked_index = None;
        for (visible_pos, palette_index) in visible.enumerate() {
            let col = visible_pos % COLUMNS;
            let row = visible_pos / COLUMNS;
            let swatch_rect = Rect::from_min_size(
                grid_rect.min + Vec2::new(col as f32 * swatch_size.x, row as f32 * swatch_size.y),
                swatch_size,
            );
            let [r, g, b] = self.entries[palette_index];
            ui.painter()
                .rect_filled(swatch_rect.shrink(0.5), 0.0, Color32::from_rgb(r, g, b));
            ui.painter().rect_stroke(
                swatch_rect.shrink(0.5),
                0.0,
                Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                StrokeKind::Inside,
            );

            let mut swatch_response =
                ui.interact(swatch_rect, response.id.with(palette_index), Sense::click());
            swatch_response =
                swatch_response.on_hover_text(swatch_tooltip(palette_index as u8, [r, g, b]));

            if swatch_response.clicked() {
                clicked_index = Some(palette_index as u8);
                if *self.selected != clicked_index {
                    *self.selected = clicked_index;
                    response.mark_changed();
                }
            }

            if swatch_response.hovered() {
                ui.painter().rect_stroke(
                    swatch_rect.shrink(0.5),
                    0.0,
                    Stroke::new(1.0, Color32::WHITE),
                    StrokeKind::Inside,
                );
            }
            if *self.selected == Some(palette_index as u8) {
                ui.painter().rect_stroke(
                    swatch_rect.shrink(0.5),
                    0.0,
                    Stroke::new(2.0, ui.visuals().selection.stroke.color),
                    StrokeKind::Inside,
                );
            }

            response = response.union(swatch_response);
        }

        PalettePickerOutput {
            response,
            clicked_index,
        }
    }
}

impl egui::Widget for PalettePicker<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

fn swatch_tooltip(index: u8, rgb: [u8; 3]) -> String {
    let [r, g, b] = rgb;
    format!(
        "#{:02X}{:02X}{:02X}\nRGB({r}, {g}, {b})\nIndex {index}",
        r, g, b
    )
}

#[cfg(test)]
mod tests {
    use super::{PaletteFilter, swatch_tooltip};

    #[test]
    fn filter_ranges_match_jill_palette_layout() {
        assert_eq!(PaletteFilter::All.index_range(), 0..256);
        assert_eq!(PaletteFilter::Ega.index_range(), 0..16);
        assert_eq!(PaletteFilter::Greyscale.index_range(), 16..24);
        assert_eq!(PaletteFilter::Vga.index_range(), 24..256);
    }

    #[test]
    fn tooltip_shows_hex_rgb_and_index() {
        assert_eq!(
            swatch_tooltip(42, [0xAB, 0xCD, 0xEF]),
            "#ABCDEF\nRGB(171, 205, 239)\nIndex 42"
        );
    }
}
