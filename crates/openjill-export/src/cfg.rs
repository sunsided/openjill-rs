//! CFG export utilities.

use crate::Row;
use openjill_data::cfg::CfgFile;
use std::fmt::Write;

/// Converts parsed `JILL1.CFG` content into tabular export rows.
///
/// **Note:** this function is not yet implemented and will panic at runtime.
/// It is retained as an internal placeholder for a future follow-up issue.
#[allow(dead_code)]
pub(crate) fn file_to_rows(_file: &CfgFile) -> Vec<Row> {
    unimplemented!("CFG export wiring lands in a follow-up issue")
}

/// Exports parsed `JILL1.CFG` high-score entries into a human-readable aligned text table.
///
/// The table includes a header row, divider row, and one row per high-score entry with
/// columns `rank`, `name`, `score` in source order (rank 1 through 10).
/// Numeric `rank` and `score` values are right-aligned; `name` is left-aligned.
pub fn scores_to_text(cfg: &CfgFile) -> String {
    let rows: Vec<[String; 3]> = cfg
        .high_scores()
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            [
                (index + 1).to_string(),
                entry.name().to_string(),
                entry.score().to_string(),
            ]
        })
        .collect();

    let headers = ["rank", "name", "score"];
    let widths = [
        max_width(headers[0], rows.iter().map(|row| row[0].as_str())),
        max_width(headers[1], rows.iter().map(|row| row[1].as_str())),
        max_width(headers[2], rows.iter().map(|row| row[2].as_str())),
    ];

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>width0$}  {:<width1$}  {:>width2$}",
        headers[0],
        headers[1],
        headers[2],
        width0 = widths[0],
        width1 = widths[1],
        width2 = widths[2],
    );
    let _ = writeln!(
        out,
        "{}  {}  {}",
        "-".repeat(widths[0]),
        "-".repeat(widths[1]),
        "-".repeat(widths[2]),
    );
    for row in &rows {
        let _ = writeln!(
            out,
            "{:>width0$}  {:<width1$}  {:>width2$}",
            row[0],
            row[1],
            row[2],
            width0 = widths[0],
            width1 = widths[1],
            width2 = widths[2],
        );
    }

    out
}

/// Exports parsed `JILL1.CFG` save-slot entries into a human-readable aligned text table.
///
/// The table includes a header row, divider row, and one row per save slot with
/// columns `slot`, `name`, `save_game_file` in source order (slots 0 through 5).
/// `jn_ext` identifies the episode prefix (for example `JN1`) and is used only for
/// documentation context; the save-file names embedded in each slot are used directly.
/// Numeric `slot` is right-aligned; `name` and `save_game_file` are left-aligned.
pub fn save_slots_to_text(cfg: &CfgFile, _jn_ext: &str) -> String {
    let rows: Vec<[String; 3]> = cfg
        .save_slots()
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            [
                index.to_string(),
                slot.name().to_string(),
                slot.save_game_file().to_string(),
            ]
        })
        .collect();

    let headers = ["slot", "name", "save_game_file"];
    let widths = [
        max_width(headers[0], rows.iter().map(|row| row[0].as_str())),
        max_width(headers[1], rows.iter().map(|row| row[1].as_str())),
        max_width(headers[2], rows.iter().map(|row| row[2].as_str())),
    ];

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:>width0$}  {:<width1$}  {:<width2$}",
        headers[0],
        headers[1],
        headers[2],
        width0 = widths[0],
        width1 = widths[1],
        width2 = widths[2],
    );
    let _ = writeln!(
        out,
        "{}  {}  {}",
        "-".repeat(widths[0]),
        "-".repeat(widths[1]),
        "-".repeat(widths[2]),
    );
    for row in &rows {
        let _ = writeln!(
            out,
            "{:>width0$}  {:<width1$}  {:<width2$}",
            row[0],
            row[1],
            row[2],
            width0 = widths[0],
            width1 = widths[1],
            width2 = widths[2],
        );
    }

    out
}

/// Computes the max width needed for a column's header and all row values.
///
/// Used by [`scores_to_text`] and [`save_slots_to_text`] so each rendered table
/// column stays aligned across all data rows. Returns `header.len()` when `values`
/// is empty.
fn max_width<'a>(header: &str, values: impl Iterator<Item = &'a str>) -> usize {
    values.fold(header.len(), |width, value| width.max(value.len()))
}

#[cfg(test)]
mod tests {
    use super::{save_slots_to_text, scores_to_text};
    use assert2::check;
    use openjill_data::cfg::CfgFile;

    /// Builds a synthetic `CfgFile` fixture with 10 high-score entries (only the
    /// first two are non-zero) and 6 save slots (only the first two are non-zero)
    /// for unit-testing the text-table exporters.
    fn fixture_cfg() -> CfgFile {
        // 254-byte CFG layout:
        //   10 × 10-byte high-score names   (bytes 0..100)
        //    20-byte hole                   (bytes 100..120)
        //   10 × i32le scores               (bytes 120..160)
        //    6 × 12-byte save names         (bytes 160..232)
        //   11 × i16le setup fields         (bytes 232..254)
        let mut bytes = vec![0u8; 254];

        // High-score names (first two have data; rest are blank/zero)
        bytes[0..5].copy_from_slice(b"ALICE");
        bytes[10..14].copy_from_slice(b"BOBB");

        // High-score scores (i32le), slots 0 and 1
        let score_base = 100 + 20; // after names + hole
        bytes[score_base..score_base + 4].copy_from_slice(&9999i32.to_le_bytes());
        bytes[score_base + 4..score_base + 8].copy_from_slice(&1234i32.to_le_bytes());

        // Save slot names
        let save_base = score_base + 40; // after 10 × i32
        bytes[save_base..save_base + 5].copy_from_slice(b"LEVEL");
        bytes[save_base + 12..save_base + 17].copy_from_slice(b"EXTRA");

        CfgFile::from_bytes(bytes, "JN1").expect("synthetic CFG should parse")
    }

    /// Unit under test: [`scores_to_text`].
    ///
    /// Invariants asserted: output contains the `rank`/`name`/`score` header,
    /// a divider row, and exactly one data row per high-score entry in source order.
    #[test]
    fn scores_to_text_emits_aligned_table_with_header_divider_and_rows() {
        let cfg = fixture_cfg();
        let text = scores_to_text(&cfg);
        let lines: Vec<&str> = text.lines().collect();

        check!(lines.len() == cfg.high_scores().len() + 2);
        check!(lines[0].contains("rank"));
        check!(lines[0].contains("name"));
        check!(lines[0].contains("score"));
        check!(lines[1].contains("---"));
        check!(lines[2].contains("ALICE"));
        check!(lines[2].contains("9999"));
        check!(lines[3].contains("BOBB"));
        check!(lines[3].contains("1234"));
    }

    /// Unit under test: [`save_slots_to_text`].
    ///
    /// Invariants asserted: output contains the `slot`/`name`/`save_game_file` header,
    /// a divider row, and exactly one data row per save slot in source order.
    #[test]
    fn save_slots_to_text_emits_aligned_table_with_header_divider_and_rows() {
        let cfg = fixture_cfg();
        let text = save_slots_to_text(&cfg, "JN1");
        let lines: Vec<&str> = text.lines().collect();

        check!(lines.len() == cfg.save_slots().len() + 2);
        check!(lines[0].contains("slot"));
        check!(lines[0].contains("name"));
        check!(lines[0].contains("save_game_file"));
        check!(lines[1].contains("---"));
        check!(lines[2].contains("LEVEL"));
        check!(lines[2].contains("JN1SAVE.0"));
        check!(lines[3].contains("EXTRA"));
        check!(lines[3].contains("JN1SAVE.1"));
    }
}
