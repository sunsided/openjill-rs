//! VCL text-entry and sound export utilities.

use openjill_data::vcl::{VclFile, VclSound};
use std::fmt::Write;

/// Uppercase hex digits used when escaping non-printable control characters.
const HEX_DIGITS: &[u8; 16] = b"0123456789ABCDEF";

/// Exports parsed `*.VCL` text entries as aligned text lines.
///
/// Each output line contains one entry in parser order with a right-aligned
/// index and control-character-escaped payload: `<index>: <payload>`.
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
            escape_text_payload(entry.text()),
            index_width = index_width
        )
        .expect("writing VCL text export into String should not fail");
    }
    out
}

/// Escapes control characters for line-oriented VCL text export.
pub fn escape_text_payload(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\0' => escaped.push_str("\\0"),
            '\r' => escaped.push_str("\\r"),
            '\n' => escaped.push_str("\\n"),
            ch if ch.is_control() => {
                let value = ch as u32;
                escaped.push_str("\\x");
                escaped.push(HEX_DIGITS[((value >> 4) & 0x0F) as usize] as char);
                escaped.push(HEX_DIGITS[(value & 0x0F) as usize] as char);
            }
            ch => escaped.push(ch),
        }
    }
    escaped
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

/// Encodes a decoded VCL sound as a 16-bit signed mono PCM WAV file.
///
/// The VCL payload is 8-bit signed PCM; each sample is scaled to 16-bit
/// (`sample << 8`) so the result plays in any standard audio editor at the
/// entry's sample rate. Useful for auditioning the original sounds to map them
/// to game events.
pub fn sound_to_wav(sound: &VclSound) -> Vec<u8> {
    /// PCM sample width emitted into the WAV.
    const BITS_PER_SAMPLE: u16 = 16;
    /// Mono output.
    const CHANNELS: u16 = 1;

    let sample_rate = u32::from(sound.frequency());
    let block_align = CHANNELS * (BITS_PER_SAMPLE / 8);
    let byte_rate = sample_rate * u32::from(block_align);
    let data_len = (sound.pcm().len() * 2) as u32;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt-chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // audio format = PCM
    out.extend_from_slice(&CHANNELS.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for &sample in sound.pcm() {
        out.extend_from_slice(&(i16::from(sample) << 8).to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{entries_to_json, entries_to_text, escape_text_payload, sound_to_wav};
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

    /// Unit under test: [`entries_to_text`].
    ///
    /// Invariants asserted: control characters are escaped so text output keeps
    /// one entry per line.
    #[test]
    fn entries_to_text_escapes_control_characters() {
        let mut bytes = vec![0; table_end()];
        write_text_entry(&mut bytes, 4, 700, 5);
        write_text_at(&mut bytes, 700, b"A\n\r\0\x1b");
        let vcl = VclFile::from_bytes(bytes).expect("fixture should parse");

        check!(entries_to_text(&vcl) == format!("4: {}\n", escape_text_payload("A\n\r\0\x1b")));
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
            json == serde_json::json!([
                {"index": 3, "payload": "HELLO"},
                {"index": 12, "payload": "A\\B"},
                {"index": 39, "payload": "DONE"},
            ])
        );
    }

    /// Unit under test: [`sound_to_wav`] emits a well-formed 16-bit mono PCM
    /// WAV with the sound's sample rate and `sample << 8` data.
    #[test]
    fn sound_to_wav_writes_16bit_mono_pcm() {
        let mut bytes = vec![0; table_end() + 3];
        // Sound slot 1: 3 bytes of PCM at the payload offset, 6000 Hz.
        let offset = table_end();
        bytes[4..8].copy_from_slice(&(offset as u32).to_le_bytes()); // offsets[1] @ 1*4
        bytes[202..204].copy_from_slice(&3u16.to_le_bytes()); // lengths[1] @ 200 + 1*2
        bytes[302..304].copy_from_slice(&6000u16.to_le_bytes()); // freqs[1] @ 300 + 1*2
        bytes[offset] = 0u8;
        bytes[offset + 1] = 127u8;
        bytes[offset + 2] = 0x80u8; // -128
        let vcl = VclFile::from_bytes(bytes).expect("fixture should parse");
        let sound = vcl.sound(1).expect("slot 1 has a sound");

        let wav = sound_to_wav(sound);

        check!(&wav[0..4] == b"RIFF");
        check!(&wav[8..12] == b"WAVE");
        check!(&wav[12..16] == b"fmt ");
        check!(&wav[36..40] == b"data");
        // 3 samples * 2 bytes = 6 data bytes; whole file = 44 + 6.
        check!(wav.len() == 50);
        check!(u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]) == 6000);
        check!(u16::from_le_bytes([wav[22], wav[23]]) == 1); // mono
        check!(u16::from_le_bytes([wav[34], wav[35]]) == 16); // bits per sample
        // First sample 0 -> 0; second 127 -> 127<<8; third -128 -> -128<<8.
        check!(i16::from_le_bytes([wav[44], wav[45]]) == 0);
        check!(i16::from_le_bytes([wav[46], wav[47]]) == 127 << 8);
        check!(i16::from_le_bytes([wav[48], wav[49]]) == -128 << 8);
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
