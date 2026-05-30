use crate::{ByteReader, ByteReaderError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Number of bytes occupied by the sound-entry tables at the start of a
/// `JILL1.VCL` file (`50` u32 offsets + `50` u16 lengths + `50` u16 frequencies).
const SOUND_ENTRY_SKIP: usize = 400;
/// Number of sound-entry slots in the `JILL1.VCL` sound tables.
const SOUND_ENTRY_COUNT: usize = 50;
/// Number of text-entry slots in the `JILL1.VCL` text offset/length tables.
const TEXT_ENTRY_COUNT: usize = 40;
/// Byte offset immediately after the contiguous table region (sound tables
/// followed by the text offset/length tables). Sound and text payloads live at
/// or beyond this offset, so a payload offset below it overlaps a table and is
/// rejected.
const TABLES_END: usize = SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (TEXT_ENTRY_COUNT * 2);

/// A decoded non-empty sound entry from a `JILL1.VCL` sound table.
///
/// The payload is 8-bit signed raw PCM (`pcm`) played at the entry's sample
/// rate (`frequency`, Hz; Jill ships sounds at ~6000 Hz). Mirrors the VCL sound
/// format documented in `docs/port/00-format-reference.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VclSound {
    /// Sample rate in Hz from the sound-frequency table.
    frequency: u16,
    /// 8-bit signed PCM samples.
    pcm: Vec<i8>,
}

impl VclSound {
    /// Returns the sample rate in Hz.
    pub fn frequency(&self) -> u16 {
        self.frequency
    }

    /// Returns the 8-bit signed PCM samples.
    pub fn pcm(&self) -> &[i8] {
        &self.pcm
    }
}

/// A decoded non-empty text entry from a `JILL1.VCL` text table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VclTextEntry {
    /// Zero-based slot index from the VCL text table.
    index: usize,
    /// Declared text length from the VCL text-length table.
    declared_length: u16,
    /// Text bytes mapped one-to-one to `char` values (`U+0000..U+00FF`).
    text: String,
    /// Source byte offset where this text entry starts in the original file.
    offset: usize,
}

impl VclTextEntry {
    /// Returns the zero-based slot index from the text table.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the declared text length from the source table.
    pub fn declared_length(&self) -> u16 {
        self.declared_length
    }

    /// Returns the decoded text payload.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the source byte offset where this text payload starts.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

/// Parsed sound and text tables from a `JILL1.VCL` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VclFile {
    /// Non-empty text entries preserved in table order.
    text_entries: Vec<VclTextEntry>,
    /// All [`SOUND_ENTRY_COUNT`] sound slots in table order; empty or
    /// out-of-range slots are `None`.
    sounds: Vec<Option<VclSound>>,
}

impl VclFile {
    /// Parses a `JILL1.VCL` file (sound + text tables) from a reader.
    ///
    /// Parsing semantics follow the documented VCL layout:
    /// - read the 400-byte sound table (50 `u32le` offsets, 50 `u16le` lengths,
    ///   50 `u16le` frequencies)
    /// - read 40 `u32le` text offsets and 40 `u16le` text lengths
    /// - materialize non-empty text entries and decode non-empty sound payloads
    ///
    /// Sound payloads are 8-bit signed PCM at each entry's sample rate. A sound
    /// slot that is empty or whose `(offset, length)` falls outside the file
    /// degrades to `None` rather than failing the parse, so a malformed sound
    /// slot never aborts parsing (and `openjill-data` stays log-free). Text
    /// payloads keep their stricter typed-error behavior.
    ///
    /// On success the reader cursor is restored to the byte immediately after
    /// the text length table, regardless of which payloads were seeked into, so
    /// the post-parse position is deterministic for chained consumers.
    pub fn parse(reader: &mut ByteReader) -> Result<Self, VclReadError> {
        let mut sound_offsets = [0u32; SOUND_ENTRY_COUNT];
        let mut sound_lengths = [0u16; SOUND_ENTRY_COUNT];
        let mut sound_frequencies = [0u16; SOUND_ENTRY_COUNT];

        for (entry_index, value) in sound_offsets.iter_mut().enumerate() {
            *value = read_u32(reader, "sound_offset", entry_index)?;
        }
        for (entry_index, value) in sound_lengths.iter_mut().enumerate() {
            *value = read_u16(reader, "sound_length", entry_index)?;
        }
        for (entry_index, value) in sound_frequencies.iter_mut().enumerate() {
            *value = read_u16(reader, "sound_frequency", entry_index)?;
        }

        let mut text_offsets = [0u32; TEXT_ENTRY_COUNT];
        let mut text_lengths = [0u16; TEXT_ENTRY_COUNT];

        for (entry_index, text_offset) in text_offsets.iter_mut().enumerate() {
            *text_offset = read_u32(reader, "text_offset", entry_index)?;
        }

        for (entry_index, text_length) in text_lengths.iter_mut().enumerate() {
            *text_length = read_u16(reader, "text_length", entry_index)?;
        }

        let post_table_offset = reader.offset();

        let mut text_entries = Vec::new();
        for entry_index in 0..TEXT_ENTRY_COUNT {
            let text_length = text_lengths[entry_index] as usize;
            if text_length == 0 {
                continue;
            }

            let text_offset = text_offsets[entry_index] as usize;
            let seek_offset = reader.offset();
            reader.seek(text_offset).map_err(|source| VclReadError {
                field: "text_offset",
                entry_index: Some(entry_index),
                offset: error_offset(&source, seek_offset),
                source,
            })?;

            let mut text = String::with_capacity(text_length);
            for _ in 0..text_length {
                text.push(char::from(read_u8(reader, "text_value", entry_index)?));
            }

            text_entries.push(VclTextEntry {
                index: entry_index,
                declared_length: text_lengths[entry_index],
                text,
                offset: text_offset,
            });
        }

        let file_len = reader.len();
        let mut sounds: Vec<Option<VclSound>> = Vec::with_capacity(SOUND_ENTRY_COUNT);
        for entry_index in 0..SOUND_ENTRY_COUNT {
            sounds.push(decode_sound(
                reader,
                sound_offsets[entry_index] as usize,
                sound_lengths[entry_index] as usize,
                sound_frequencies[entry_index],
                file_len,
            ));
        }

        reader
            .seek(post_table_offset)
            .map_err(|source| VclReadError {
                field: "post_table_restore",
                entry_index: None,
                offset: error_offset(&source, post_table_offset),
                source,
            })?;

        Ok(Self {
            text_entries,
            sounds,
        })
    }

    /// Parses a `VclFile` directly from in-memory bytes.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, VclReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader)
    }

    /// Returns all parsed non-empty text entries in table order.
    pub fn text_entries(&self) -> &[VclTextEntry] {
        &self.text_entries
    }

    /// Returns the number of parsed non-empty text entries.
    pub fn text_entry_count(&self) -> usize {
        self.text_entries.len()
    }

    /// Returns the decoded sound at table slot `index` (`0..SOUND_ENTRY_COUNT`),
    /// or `None` when the slot is empty or its payload was out of range.
    pub fn sound(&self, index: usize) -> Option<&VclSound> {
        self.sounds.get(index).and_then(Option::as_ref)
    }

    /// Returns all sound slots in table order; empty or out-of-range slots are
    /// `None`.
    pub fn sounds(&self) -> &[Option<VclSound>] {
        &self.sounds
    }
}

/// Decodes one VCL sound payload, or `None` for an empty / out-of-range slot.
///
/// The payload is `length` bytes of 8-bit signed PCM at `offset`. A slot is
/// `None` when it is empty (`length == 0`), overlaps the table region (offset
/// below [`TABLES_END`]), or runs past the end of the file. Such slots degrade
/// silently rather than failing the parse, keeping `openjill-data` log-free; the
/// caller decides whether a missing sound is worth reporting.
fn decode_sound(
    reader: &mut ByteReader,
    offset: usize,
    length: usize,
    frequency: u16,
    file_len: usize,
) -> Option<VclSound> {
    if length == 0 || offset < TABLES_END {
        return None;
    }
    if offset.checked_add(length)? > file_len {
        return None;
    }

    reader.seek(offset).ok()?;
    let mut pcm = Vec::with_capacity(length);
    for _ in 0..length {
        pcm.push(reader.read_i8().ok()?);
    }
    Some(VclSound { frequency, pcm })
}

/// Error returned when parsing a `JILL1.VCL` table region fails (the sound
/// offset/length/frequency tables or the text offset/length tables). Decoding
/// individual sound payloads never produces this error - a bad sound slot
/// degrades to `None` instead.
#[derive(Debug, Eq, PartialEq)]
pub struct VclReadError {
    /// Name of the field being parsed when the failure occurred.
    pub field: &'static str,
    /// Optional table index for entry-scoped failures.
    pub entry_index: Option<usize>,
    /// Source offset associated with the parse failure.
    pub offset: usize,
    /// Underlying byte-reader failure.
    source: ByteReaderError,
}

impl Display for VclReadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(entry_index) = self.entry_index {
            write!(
                f,
                "failed to parse VCL field '{}' for entry {} at offset {}: {}",
                self.field, entry_index, self.offset, self.source
            )
        } else {
            write!(
                f,
                "failed to parse VCL field '{}' at offset {}: {}",
                self.field, self.offset, self.source
            )
        }
    }
}

impl Error for VclReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Reads a single `u8` field and wraps reader errors with VCL parse context.
fn read_u8(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: usize,
) -> Result<u8, VclReadError> {
    let fallback_offset = reader.offset();
    reader.read_u8().map_err(|source| VclReadError {
        field,
        entry_index: Some(entry_index),
        offset: error_offset(&source, fallback_offset),
        source,
    })
}

/// Reads a single `u16le` field and wraps reader errors with VCL parse context.
fn read_u16(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: usize,
) -> Result<u16, VclReadError> {
    let fallback_offset = reader.offset();
    reader.read_u16_le().map_err(|source| VclReadError {
        field,
        entry_index: Some(entry_index),
        offset: error_offset(&source, fallback_offset),
        source,
    })
}

/// Reads a single `u32le` field and wraps reader errors with VCL parse context.
fn read_u32(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: usize,
) -> Result<u32, VclReadError> {
    let fallback_offset = reader.offset();
    reader.read_u32_le().map_err(|source| VclReadError {
        field,
        entry_index: Some(entry_index),
        offset: error_offset(&source, fallback_offset),
        source,
    })
}

/// Chooses the most useful parse-failure offset to report for a reader error.
fn error_offset(source: &ByteReaderError, lower_bound_offset: usize) -> usize {
    match source {
        ByteReaderError::UnexpectedEof { offset, .. }
        | ByteReaderError::OffsetOverflow { offset, .. } => *offset,
        ByteReaderError::InvalidSeek { requested, .. } => *requested,
    }
    .max(lower_bound_offset)
}

#[cfg(test)]
mod tests {
    use super::{SOUND_ENTRY_COUNT, SOUND_ENTRY_SKIP, TEXT_ENTRY_COUNT, VclFile, VclReadError};
    use crate::{ByteReader, ByteReaderError};
    use assert2::check;

    /// Unit under test: `VclFile::parse` materialization of the text table.
    ///
    /// Preconditions: a synthetic buffer sized to the end of the offset/length
    /// tables, with four entries seeded — two empty (lengths zero at indices
    /// 0 and 39) and two non-empty (`HELLO` at offset 700, a four-byte mixed
    /// payload including a zero byte and a high-byte at offset 705).
    ///
    /// Invariants asserted: empty-length entries are skipped, only the two
    /// non-empty entries materialize, their preserved offsets equal the
    /// configured source offsets, and the decoded text uses the
    /// byte-to-`U+0000..U+00FF` mapping (so `0x00` becomes the NUL code point
    /// and `0xe9` becomes `é`).
    #[test]
    fn parses_sparse_text_entries_and_skips_empty_ones() {
        let mut bytes = vec![0; table_end()];

        write_text_entry(&mut bytes, 0, 700, 0);
        // These sparse indices (5 and 12) are asserted below via `index()`.
        write_text_entry(&mut bytes, 5, 700, 5);
        write_text_entry(&mut bytes, 12, 705, 4);
        write_text_entry(&mut bytes, 39, 709, 0);

        write_text_at(&mut bytes, 700, b"HELLO");
        write_text_at(&mut bytes, 705, &[0x41, 0x00, 0xe9, 0x5a]);

        let vcl = VclFile::from_bytes(bytes).expect("VCL parse should succeed");

        check!(vcl.text_entry_count() == 2);
        check!(vcl.text_entries()[0].index() == 5);
        check!(vcl.text_entries()[0].offset() == 700);
        check!(vcl.text_entries()[0].declared_length() == 5);
        check!(vcl.text_entries()[0].text() == "HELLO");
        check!(vcl.text_entries()[1].index() == 12);
        check!(vcl.text_entries()[1].offset() == 705);
        check!(vcl.text_entries()[1].declared_length() == 4);
        check!(vcl.text_entries()[1].text() == "A\0éZ");
    }

    /// Unit under test: `VclReadError` reporting when the text-offset table
    /// is truncated.
    ///
    /// Preconditions: a buffer that covers the 400-byte sound-entry skip plus
    /// three additional bytes — enough to start, but not finish, the first
    /// `u32le` in the offset table.
    ///
    /// Invariants asserted: parsing fails with `field == "text_offset"`,
    /// `entry_index == Some(0)`, the reported offset equals the start of the
    /// failing read (i.e. `SOUND_ENTRY_SKIP`), and the underlying error is
    /// the expected `UnexpectedEof` for a 4-byte little-endian read.
    #[test]
    fn includes_failing_offset_when_text_offset_table_is_truncated() {
        let bytes = vec![0; SOUND_ENTRY_SKIP + 3];

        check!(
            let Err(err) = VclFile::from_bytes(bytes)
                && err == VclReadError {
                    field: "text_offset",
                    entry_index: Some(0),
                    offset: SOUND_ENTRY_SKIP,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read unsigned 32-bit little-endian integer",
                        offset: SOUND_ENTRY_SKIP,
                        requested: 4,
                        len: SOUND_ENTRY_SKIP + 3,
                    },
                }
        );
    }

    /// Unit under test: `VclReadError` reporting when a text payload is
    /// truncated.
    ///
    /// Preconditions: a buffer sized to the end of the tables, with entry 2
    /// declaring a length of 2 starting at offset 700, but only one byte of
    /// payload (`X`) actually written there — so the second payload byte
    /// runs past EOF.
    ///
    /// Invariants asserted: parsing fails with `field == "text_value"`,
    /// `entry_index == Some(2)`, the reported offset equals the byte just
    /// past the truncated payload (`701`), and the underlying error is the
    /// expected `UnexpectedEof` for a 1-byte read.
    #[test]
    fn includes_failing_offset_when_text_bytes_are_truncated() {
        let mut bytes = vec![0; table_end()];
        write_text_entry(&mut bytes, 2, 700, 2);
        write_text_at(&mut bytes, 700, b"X");

        check!(
            let Err(err) = VclFile::from_bytes(bytes)
                && err == VclReadError {
                    field: "text_value",
                    entry_index: Some(2),
                    offset: 701,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read unsigned 8-bit integer",
                        offset: 701,
                        requested: 1,
                        len: 701,
                    },
                }
        );
    }

    /// Unit under test: deterministic post-parse cursor position guaranteed
    /// by `VclFile::parse`.
    ///
    /// Preconditions: a buffer sized to the end of the tables with two
    /// non-empty entries (lengths 5 and 3) whose payloads are written at
    /// offsets 700 and 705 — both far enough into the file that the parser
    /// has to seek away from the table region to read them.
    ///
    /// Invariants asserted: after a successful parse, the reader's cursor is
    /// restored to the byte immediately after the length table
    /// (`table_end()`) regardless of the seeks performed for the text
    /// payloads, so chained consumers see a deterministic position.
    #[test]
    fn parse_leaves_reader_at_deterministic_position_past_tables() {
        let mut bytes = vec![0; table_end()];

        write_text_entry(&mut bytes, 0, 700, 5);
        write_text_entry(&mut bytes, 7, 705, 3);
        write_text_at(&mut bytes, 700, b"HELLO");
        write_text_at(&mut bytes, 705, b"FOO");

        let mut reader = ByteReader::from_bytes(bytes);
        VclFile::parse(&mut reader).expect("VCL parse should succeed");

        check!(reader.offset() == table_end());
    }

    /// Unit under test: `VclFile::parse` decoding of the sound table - sparse
    /// non-empty slots, the signed-PCM byte mapping, and `sound`/`sounds`.
    ///
    /// Preconditions: slot 3 declares a 4-byte payload at offset 800 (freq
    /// 6000) whose bytes span the signed extremes; slot 7 declares a payload
    /// offset but a zero length.
    ///
    /// Invariants asserted: slot 3 decodes to the expected `i8` samples at the
    /// declared frequency, the zero-length slot 7 and the never-written slot 0
    /// are `None`, and `sounds()` exposes all 50 slots.
    #[test]
    fn parses_sparse_sound_entries_and_decodes_signed_pcm() {
        let mut bytes = vec![0; table_end()];
        write_sound_entry(&mut bytes, 3, 800, 4, 6000);
        write_sound_entry(&mut bytes, 7, 900, 0, 6000);
        // 0x00 -> 0, 0x7f -> 127, 0x80 -> -128, 0xff -> -1.
        write_text_at(&mut bytes, 800, &[0x00, 0x7f, 0x80, 0xff]);

        let vcl = VclFile::from_bytes(bytes).expect("VCL parse should succeed");

        let sound = vcl.sound(3).expect("slot 3 must decode to a sound");
        check!(sound.frequency() == 6000);
        check!(sound.pcm() == [0i8, 127, -128, -1]);
        check!(vcl.sound(7).is_none());
        check!(vcl.sound(0).is_none());
        check!(vcl.sounds().len() == SOUND_ENTRY_COUNT);
    }

    /// Unit under test: out-of-range sound slots degrade to `None` instead of
    /// failing the parse, keeping `openjill-data` log-free. Covers both a
    /// payload that overlaps the table region and one that runs past EOF.
    #[test]
    fn out_of_range_sound_slot_degrades_to_none() {
        // Buffer extends past the tables so a payload offset can sit beyond them.
        let mut bytes = vec![0; table_end() + 64];

        let file_len = bytes.len() as u32;
        // (a) Offset points inside the table region -> rejected.
        write_sound_entry(&mut bytes, 1, (table_end() as u32) - 1, 4, 6000);
        // (b) Offset is past the tables but offset + length runs past EOF.
        write_sound_entry(&mut bytes, 2, file_len - 2, 8, 6000);

        let vcl =
            VclFile::from_bytes(bytes).expect("a malformed sound slot must not fail the parse");
        check!(vcl.sound(1).is_none());
        check!(vcl.sound(2).is_none());
    }

    /// Returns the byte offset immediately after the end of the offset and
    /// length tables for a synthetic VCL fixture (sound skip plus 40 `u32`
    /// offsets plus 40 `u16` lengths).
    fn table_end() -> usize {
        SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (TEXT_ENTRY_COUNT * 2)
    }

    /// Writes a `(offset, length)` pair into the offset and length tables of
    /// a synthetic VCL fixture at slot `index`.
    fn write_text_entry(bytes: &mut [u8], index: usize, offset: u32, length: u16) {
        let offset_pos = SOUND_ENTRY_SKIP + (index * 4);
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());

        let length_pos = SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (index * 2);
        bytes[length_pos..length_pos + 2].copy_from_slice(&length.to_le_bytes());
    }

    /// Writes a sound `(offset, length, frequency)` triple into the sound
    /// offset / length / frequency tables of a synthetic VCL fixture at slot
    /// `index`.
    fn write_sound_entry(bytes: &mut [u8], index: usize, offset: u32, length: u16, frequency: u16) {
        // Sound tables are laid out offsets (u32 x N), lengths (u16 x N), then
        // frequencies (u16 x N); derive each base from SOUND_ENTRY_COUNT so the
        // fixture tracks the parser's layout constants.
        let lengths_base = SOUND_ENTRY_COUNT * 4;
        let frequencies_base = lengths_base + (SOUND_ENTRY_COUNT * 2);

        let offset_pos = index * 4;
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());

        let length_pos = lengths_base + (index * 2);
        bytes[length_pos..length_pos + 2].copy_from_slice(&length.to_le_bytes());

        let frequency_pos = frequencies_base + (index * 2);
        bytes[frequency_pos..frequency_pos + 2].copy_from_slice(&frequency.to_le_bytes());
    }

    /// Writes a text payload into the synthetic fixture at the given offset,
    /// growing the buffer with zero padding when the payload runs past the
    /// current end.
    fn write_text_at(bytes: &mut Vec<u8>, offset: usize, text: &[u8]) {
        let end = offset + text.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset..end].copy_from_slice(text);
    }
}
