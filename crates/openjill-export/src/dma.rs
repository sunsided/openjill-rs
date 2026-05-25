//! DMA table export utilities.

use crate::Row;
use openjill_data::dma::DmaFile;
use std::fmt::Write;

/// REVERSE-ENGINEERED: Known DMA/JN `iFlags` bits mapped to symbolic names.
///
/// These names mirror the `openjill_data::dma` flag helper semantics and the
/// repository's Jill format reference
/// (`docs/port/00-format-reference.md`, DMA `iFlags` table).
const FLAG_NAMES: &[(u16, &str)] = &[
    (0x0001, "PLAYERTHRU"),
    (0x0002, "NOTSTAIR"),
    (0x0004, "NOTVINE"),
    (0x0008, "MSGTOUCH"),
    (0x0010, "MSGDRAW"),
    (0x0020, "MSGUPDATE"),
    (0x0040, "INSIDE"),
    (0x0080, "FRONT"),
    (0x0200, "BACK/TINY"),
    (0x0800, "KILLABLE"),
    (0x1000, "FIREBALL"),
    (0x2000, "WATER"),
    (0x4000, "WEAPON"),
];

/// Converts parsed `JILL.DMA` metadata into tabular export rows.
pub fn file_to_rows(_file: &DmaFile) -> Vec<Row> {
    unimplemented!("DMA export wiring lands in a follow-up issue")
}

/// Exports parsed `JILL.DMA` entries into CSV with one row per entry.
///
/// The output header is `map_code,tileset,tile,flags,flag_names`.
/// `flag_names` is emitted as `|`-separated symbolic names (for example
/// `PLAYERTHRU|MSGTOUCH`) or `-` when no known flag bits are set.
pub fn table_to_csv(dma: &DmaFile) -> String {
    let mut out = String::from("map_code,tileset,tile,flags,flag_names\n");

    for entry in dma.entries() {
        let _ = writeln!(
            out,
            "{},{},{},{},{}",
            entry.map_code(),
            entry.tileset(),
            entry.tile(),
            entry.flags(),
            decode_flag_names(entry.flags())
        );
    }

    out
}

/// Exports parsed `JILL.DMA` entries into a human-readable aligned text table.
///
/// The table includes a header row, divider row, and one row per entry with
/// columns `map_code`, `tileset`, `tile`, `flags`, `flag_names`. Numeric
/// `map_code`/`flags` values are emitted in uppercase hex (`0x0102`).
pub fn table_to_text(dma: &DmaFile) -> String {
    let rows: Vec<[String; 5]> = dma
        .entries()
        .iter()
        .map(|entry| {
            [
                format!("0x{:04X}", entry.map_code()),
                entry.tileset().to_string(),
                entry.tile().to_string(),
                format!("0x{:04X}", entry.flags()),
                decode_flag_names(entry.flags()),
            ]
        })
        .collect();

    let headers = ["map_code", "tileset", "tile", "flags", "flag_names"];
    let widths = [
        max_width(headers[0], rows.iter().map(|row| row[0].as_str())),
        max_width(headers[1], rows.iter().map(|row| row[1].as_str())),
        max_width(headers[2], rows.iter().map(|row| row[2].as_str())),
        max_width(headers[3], rows.iter().map(|row| row[3].as_str())),
        max_width(headers[4], rows.iter().map(|row| row[4].as_str())),
    ];

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>width0$}  {:>width1$}  {:>width2$}  {:>width3$}  {:<width4$}",
        headers[0],
        headers[1],
        headers[2],
        headers[3],
        headers[4],
        width0 = widths[0],
        width1 = widths[1],
        width2 = widths[2],
        width3 = widths[3],
        width4 = widths[4]
    );
    let _ = writeln!(
        out,
        "{}  {}  {}  {}  {}",
        "-".repeat(widths[0]),
        "-".repeat(widths[1]),
        "-".repeat(widths[2]),
        "-".repeat(widths[3]),
        "-".repeat(widths[4])
    );

    for row in rows {
        let _ = writeln!(
            out,
            "{:>width0$}  {:>width1$}  {:>width2$}  {:>width3$}  {:<width4$}",
            row[0],
            row[1],
            row[2],
            row[3],
            row[4],
            width0 = widths[0],
            width1 = widths[1],
            width2 = widths[2],
            width3 = widths[3],
            width4 = widths[4]
        );
    }

    out
}

/// Decodes raw DMA flag bits into `|`-separated symbolic names.
///
/// This helper is shared by both CSV and text exports so both renderings use
/// identical human-readable names for the same raw flag bitmask. Returns `-`
/// when no known flag bits are set.
fn decode_flag_names(flags: u16) -> String {
    let names: Vec<&str> = FLAG_NAMES
        .iter()
        .filter_map(|(bit, name)| ((flags & bit) != 0).then_some(*name))
        .collect();

    if names.is_empty() {
        "-".to_string()
    } else {
        names.join("|")
    }
}

/// Computes the max width needed for a column's header and all row values.
///
/// Used by [`table_to_text`] so each rendered table column stays aligned across
/// all data rows. Returns `header.len()` when `values` is empty.
fn max_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.fold(header.len(), |width, value| width.max(value.len()))
}

#[cfg(test)]
mod tests {
    use super::{table_to_csv, table_to_text};
    use assert2::check;
    use openjill_data::dma::DmaFile;

    /// Unit under test: [`table_to_csv`].
    ///
    /// Invariants asserted: CSV output uses the requested columns and one row
    /// per DMA entry.
    #[test]
    fn table_to_csv_uses_expected_columns_and_rows() {
        let dma = fixture_dma();
        let csv = table_to_csv(&dma);
        let mut lines = csv.lines();

        check!(lines.next() == Some("map_code,tileset,tile,flags,flag_names"));
        check!(lines.next() == Some("258,8,3,57,PLAYERTHRU|MSGTOUCH|MSGDRAW|MSGUPDATE"));
        check!(lines.next() == Some("772,2,64,6,NOTSTAIR|NOTVINE"));
        check!(lines.next().is_none());
    }

    /// Unit under test: [`table_to_csv`].
    ///
    /// Invariants asserted: parsing exported CSV lines (excluding header)
    /// round-trips the same number of rows as `DmaFile::entries().len()`.
    #[test]
    fn table_to_csv_round_trips_row_count() {
        let dma = fixture_dma();
        let csv = table_to_csv(&dma);

        let parsed_rows = csv
            .lines()
            .skip(1)
            .filter(|line| !line.trim().is_empty())
            .count();

        check!(parsed_rows == dma.entries().len());
    }

    /// Unit under test: [`table_to_text`].
    ///
    /// Invariants asserted: output is column-aligned text with header, divider,
    /// and one line per entry.
    #[test]
    fn table_to_text_emits_human_readable_table() {
        let dma = fixture_dma();
        let text = table_to_text(&dma);
        let lines: Vec<&str> = text.lines().collect();

        check!(lines.len() == dma.entries().len() + 2);
        check!(lines[0].contains("map_code"));
        check!(lines[0].contains("flag_names"));
        check!(lines[1].contains("---"));
        check!(lines[2].contains("0x0102"));
        check!(lines[3].contains("0x0304"));
    }

    /// Builds a valid synthetic two-entry `JILL.DMA` fixture.
    ///
    /// Entry #1 uses map code `0x0102` and flags `0x0039`
    /// (`PLAYERTHRU|MSGTOUCH|MSGDRAW|MSGUPDATE`); entry #2 uses map code
    /// `0x0304` and flags `0x0006` (`NOTSTAIR|NOTVINE`).
    fn fixture_dma() -> DmaFile {
        let mut bytes = Vec::new();
        bytes.extend(0x0102u16.to_le_bytes());
        bytes.push(0x03);
        bytes.push(0x08);
        bytes.extend(0x0039u16.to_le_bytes());
        bytes.push(5);
        bytes.extend(b"FLOOR");

        bytes.extend(0x0304u16.to_le_bytes());
        bytes.push(0x40);
        bytes.push(0x02);
        bytes.extend(0x0006u16.to_le_bytes());
        bytes.push(3);
        bytes.extend(b"LAD");

        DmaFile::from_bytes(bytes).expect("synthetic DMA should parse")
    }
}
