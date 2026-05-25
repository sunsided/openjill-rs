//! VCL text-entry export utilities.

use openjill_data::vcl::VclFile;
use std::fmt::Write;

/// Exports parsed `*.VCL` text entries as aligned text lines.
///
/// Each output line contains one entry in parser order with a right-aligned
/// index and decoded payload: `<index>: <payload>`.
pub fn entries_to_text(vcl: &VclFile) -> String {
    let index_width = vcl
        .text_entries()
        .iter()
        .map(|entry| entry.index())
        .max()
        .unwrap_or(0)
        .to_string()
        .len()
        .max(1);

    let mut out = String::new();
    for entry in vcl.text_entries() {
        writeln!(
            out,
            "{:>index_width$}: {}",
            entry.index(),
            entry.text(),
            index_width = index_width
        )
        .expect("writing VCL text export into String should not fail");
    }
    out
}

/// Exports parsed `*.VCL` text entries as JSON (`[{index,payload}, ...]`).
pub fn entries_to_json(vcl: &VclFile) -> String {
    let entries = vcl
        .text_entries()
        .iter()
        .map(|entry| {
            serde_json::json!({
                "index": entry.index(),
                "payload": entry.text(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&entries)
        .unwrap_or_else(|error| panic!("failed to serialize VCL entries to JSON: {error}"))
}

/// Backward-compatible alias for text rendering.
pub fn file_to_string(file: &VclFile) -> String {
    entries_to_text(file)
}

#[cfg(test)]
mod tests {
    use super::{entries_to_json, entries_to_text};
    use assert2::check;
    use openjill_data::vcl::VclFile;

    /// REVERSE-ENGINEERED: `JILL1.VCL` starts with a 400-byte sound-entry area
    /// before text offset/length tables.
    const SOUND_ENTRY_SKIP: usize = 400;
    /// REVERSE-ENGINEERED: `JILL1.VCL` text offset/length tables each have
    /// 40 entry slots.
    const TEXT_ENTRY_COUNT: usize = 40;

    /// Unit under test: [`entries_to_text`].
    ///
    /// Invariants asserted: index prefixes are right-aligned and each payload
    /// is emitted on its own line in parser order.
    #[test]
    fn entries_to_text_aligns_indexes_and_preserves_payload() {
        let vcl = fixture_vcl();
        let text = entries_to_text(&vcl);
        let lines: Vec<&str> = text.lines().collect();

        check!(lines == vec![" 3: HELLO", "12: A\\B", "39: DONE"]);
    }

    /// Unit under test: [`entries_to_json`].
    ///
    /// Invariants asserted: JSON output is an array of objects with
    /// `{index,payload}` values in parser order.
    #[test]
    fn entries_to_json_emits_index_and_payload() {
        let vcl = fixture_vcl();
        let json: serde_json::Value =
            serde_json::from_str(&entries_to_json(&vcl)).expect("json output should parse");

        check!(
            json
                == serde_json::json!([
                    {"index": 3, "payload": "HELLO"},
                    {"index": 12, "payload": "A\\B"},
                    {"index": 39, "payload": "DONE"},
                ])
        );
    }

    /// Builds a synthetic `VclFile` fixture with three non-empty text entries
    /// at indices 3, 12, and 39.
    fn fixture_vcl() -> VclFile {
        let mut bytes = vec![0; table_end()];
        write_text_entry(&mut bytes, 3, 700, 5);
        write_text_entry(&mut bytes, 12, 705, 3);
        write_text_entry(&mut bytes, 39, 708, 4);
        write_text_at(&mut bytes, 700, b"HELLO");
        write_text_at(&mut bytes, 705, b"A\\B");
        write_text_at(&mut bytes, 708, b"DONE");
        VclFile::from_bytes(bytes).expect("fixture should parse")
    }

    /// Returns the byte offset immediately after the text offset/length tables
    /// in a synthetic `JILL1.VCL` fixture.
    fn table_end() -> usize {
        SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (TEXT_ENTRY_COUNT * 2)
    }

    /// Writes one `(offset,length)` record into the fixture's text-entry tables
    /// at slot `index`.
    fn write_text_entry(bytes: &mut [u8], index: usize, offset: u32, length: u16) {
        let offset_pos = SOUND_ENTRY_SKIP + (index * 4);
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());

        let length_pos = SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (index * 2);
        bytes[length_pos..length_pos + 2].copy_from_slice(&length.to_le_bytes());
    }

    /// Writes raw text bytes into the fixture at `offset`, growing the buffer
    /// with zero padding as needed.
    fn write_text_at(bytes: &mut Vec<u8>, offset: usize, text: &[u8]) {
        let end = offset + text.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset..end].copy_from_slice(text);
    }
}
