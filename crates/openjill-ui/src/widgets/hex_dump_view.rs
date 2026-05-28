use egui::text::LayoutJob;
use egui::{Color32, FontId, Response, Sense, TextFormat, Ui};
use std::ops::Range;

/// Number of hex columns used by [`HexDumpView`] when no override is set.
pub const DEFAULT_BYTES_PER_ROW: usize = 16;

/// Output from [`HexDumpView::show`].
pub struct HexDumpViewOutput {
    /// Combined egui response for the scroll area contents.
    pub response: Response,
    /// Byte offset under the pointer this frame, if any.
    pub hovered_offset: Option<usize>,
    /// Byte offset clicked this frame, if any.
    pub clicked_offset: Option<usize>,
}

/// Read-only egui widget rendering a classic hex dump of a byte slice.
///
/// Each row shows an offset gutter, `bytes_per_row` hex columns, and an ASCII
/// gutter. An optional selected byte range is highlighted in both the hex and
/// ASCII gutters. The widget owns a vertical [`egui::ScrollArea`] and renders
/// only the visible rows, so it stays cheap on large buffers.
pub struct HexDumpView<'a> {
    data: &'a [u8],
    bytes_per_row: usize,
    base_offset: usize,
    selection: Option<Range<usize>>,
}

impl<'a> HexDumpView<'a> {
    /// Creates a hex dump view over `data` with the default layout.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            bytes_per_row: DEFAULT_BYTES_PER_ROW,
            base_offset: 0,
            selection: None,
        }
    }

    /// Sets the number of bytes shown per row (clamped to at least 1).
    pub fn bytes_per_row(mut self, bytes_per_row: usize) -> Self {
        self.bytes_per_row = bytes_per_row.max(1);
        self
    }

    /// Sets the offset value printed for the first byte (offsets are display
    /// only and do not change which slice bytes are shown).
    pub fn base_offset(mut self, base_offset: usize) -> Self {
        self.base_offset = base_offset;
        self
    }

    /// Highlights a byte range (indices into `data`) in both gutters.
    pub fn selection(mut self, selection: Range<usize>) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Adds the widget to a UI and returns the response plus pointer offsets.
    pub fn show(self, ui: &mut Ui) -> HexDumpViewOutput {
        let font = FontId::monospace(14.0);
        // Measure monospace metrics from a single-glyph probe galley so the
        // scroll area can virtualize rows and pointer hits map to columns.
        let probe =
            ui.painter()
                .layout_no_wrap("0".to_string(), font.clone(), Color32::PLACEHOLDER);
        let row_height = probe.size().y;
        let char_width = probe.size().x;
        let total_rows = row_count(self.data.len(), self.bytes_per_row);
        let layout = ColumnLayout::new(self.bytes_per_row);

        let mut hovered_offset = None;
        let mut clicked_offset = None;

        let scroll_output = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(ui, row_height, total_rows, |ui, row_range| {
                let mut response = ui.allocate_response(egui::Vec2::ZERO, Sense::click());
                for row in row_range {
                    let start = row * self.bytes_per_row;
                    let end = (start + self.bytes_per_row).min(self.data.len());
                    let job = self.layout_row(start, &self.data[start..end], &font);
                    let galley = ui.painter().layout_job(job);
                    let (rect, row_response) =
                        ui.allocate_exact_size(galley.size(), Sense::click());
                    ui.painter().galley(rect.min, galley, Color32::PLACEHOLDER);

                    if let Some(pos) = row_response
                        .hover_pos()
                        .or(row_response.interact_pointer_pos())
                    {
                        let col = ((pos.x - rect.min.x) / char_width).floor();
                        if col >= 0.0
                            && let Some(byte) =
                                layout.byte_index_at_column(col as usize, start, end)
                        {
                            if row_response.hovered() {
                                hovered_offset = Some(byte);
                            }
                            if row_response.clicked() {
                                clicked_offset = Some(byte);
                            }
                        }
                    }
                    response = response.union(row_response);
                }
                response
            });

        HexDumpViewOutput {
            response: scroll_output.inner,
            hovered_offset,
            clicked_offset,
        }
    }

    /// Builds the colored [`LayoutJob`] for a single row.
    fn layout_row(&self, start: usize, row: &[u8], font: &FontId) -> LayoutJob {
        let mut job = LayoutJob::default();
        let normal = TextFormat::simple(font.clone(), Color32::PLACEHOLDER);
        let mut selected = normal.clone();
        selected.background = Color32::from_rgb(0x35, 0x4a, 0x6b);

        let fmt_for = |index: usize| -> &TextFormat {
            match &self.selection {
                Some(sel) if sel.contains(&index) => &selected,
                _ => &normal,
            }
        };

        job.append(
            &format!("{:08X}  ", self.base_offset + start),
            0.0,
            normal.clone(),
        );

        for col in 0..self.bytes_per_row {
            match row.get(col) {
                Some(byte) => {
                    job.append(&format!("{byte:02X}"), 0.0, fmt_for(start + col).clone());
                    job.append(" ", 0.0, normal.clone());
                }
                None => job.append("   ", 0.0, normal.clone()),
            }
        }

        job.append(" ", 0.0, normal.clone());

        for (col, byte) in row.iter().enumerate() {
            let glyph = ascii_glyph(*byte);
            job.append(&glyph.to_string(), 0.0, fmt_for(start + col).clone());
        }

        job
    }
}

impl egui::Widget for HexDumpView<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        self.show(ui).response
    }
}

/// Returns the printable ASCII glyph for a byte, or `.` for non-printables.
fn ascii_glyph(byte: u8) -> char {
    if (0x20..=0x7e).contains(&byte) {
        byte as char
    } else {
        '.'
    }
}

/// Number of rows needed to show `len` bytes at `bytes_per_row` per row.
fn row_count(len: usize, bytes_per_row: usize) -> usize {
    if bytes_per_row == 0 {
        0
    } else {
        len.div_ceil(bytes_per_row)
    }
}

/// Fixed character-column geometry of one rendered hex-dump row.
///
/// Columns are counted in monospace characters from the start of the line:
/// an 8-char offset, two spaces, then `3 * bytes_per_row` hex characters, one
/// separator space, and finally `bytes_per_row` ASCII characters.
struct ColumnLayout {
    bytes_per_row: usize,
    hex_start: usize,
    ascii_start: usize,
}

impl ColumnLayout {
    fn new(bytes_per_row: usize) -> Self {
        let hex_start = 10; // 8 offset digits + 2 spaces.
        let ascii_start = hex_start + bytes_per_row * 3 + 1; // hex block + 1 separator space.
        Self {
            bytes_per_row,
            hex_start,
            ascii_start,
        }
    }

    /// Maps a character column to the byte index it represents, or `None` when
    /// the column falls in a gutter or past the row's populated bytes.
    ///
    /// `start` is the byte index of the row's first byte and `end` is one past
    /// its last populated byte.
    fn byte_index_at_column(&self, col: usize, start: usize, end: usize) -> Option<usize> {
        let local = if col >= self.ascii_start {
            col - self.ascii_start
        } else if col >= self.hex_start {
            (col - self.hex_start) / 3
        } else {
            return None;
        };
        if local >= self.bytes_per_row {
            return None;
        }
        let byte = start + local;
        (byte < end).then_some(byte)
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnLayout, ascii_glyph, row_count};

    #[test]
    fn row_count_rounds_up() {
        assert_eq!(row_count(0, 16), 0);
        assert_eq!(row_count(1, 16), 1);
        assert_eq!(row_count(16, 16), 1);
        assert_eq!(row_count(17, 16), 2);
        assert_eq!(row_count(10, 0), 0);
    }

    #[test]
    fn ascii_glyph_maps_printable_and_control() {
        assert_eq!(ascii_glyph(b'A'), 'A');
        assert_eq!(ascii_glyph(b' '), ' ');
        assert_eq!(ascii_glyph(0x7e), '~');
        assert_eq!(ascii_glyph(0x00), '.');
        assert_eq!(ascii_glyph(0x7f), '.');
        assert_eq!(ascii_glyph(0xff), '.');
    }

    #[test]
    fn column_layout_maps_hex_and_ascii_columns() {
        let layout = ColumnLayout::new(16);
        // Offset gutter and separators map to nothing.
        assert_eq!(layout.byte_index_at_column(0, 0, 16), None);
        assert_eq!(layout.byte_index_at_column(9, 0, 16), None);
        // First hex byte starts at column 10.
        assert_eq!(layout.byte_index_at_column(10, 0, 16), Some(0));
        assert_eq!(layout.byte_index_at_column(11, 0, 16), Some(0));
        // Second hex byte at columns 13/14.
        assert_eq!(layout.byte_index_at_column(13, 0, 16), Some(1));
        // ASCII region: 10 + 16*3 + 1 = 59.
        assert_eq!(layout.byte_index_at_column(59, 0, 16), Some(0));
        assert_eq!(layout.byte_index_at_column(60, 0, 16), Some(1));
    }

    #[test]
    fn column_layout_respects_row_start_and_end() {
        let layout = ColumnLayout::new(16);
        // Row that starts at byte 32 maps column 10 to byte 32.
        assert_eq!(layout.byte_index_at_column(10, 32, 48), Some(32));
        // Past the populated end returns None (partial final row).
        assert_eq!(layout.byte_index_at_column(10 + 3 * 4, 32, 35), None);
    }
}
