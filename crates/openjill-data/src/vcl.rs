use crate::{ByteReader, ByteReaderError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Number of bytes occupied by the sound-entry area at the start of a `JILL1.VCL` file.
const SOUND_ENTRY_SKIP: usize = 400;
/// Number of text-entry slots in the `JILL1.VCL` text offset/length tables.
const TEXT_ENTRY_COUNT: usize = 40;

/// A decoded non-empty text entry from a `JILL1.VCL` text table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VclTextEntry {
    /// Text bytes mapped one-to-one to `char` values (`U+0000..U+00FF`).
    text: String,
    /// Source byte offset where this text entry starts in the original file.
    offset: usize,
}

impl VclTextEntry {
    /// Returns the decoded text payload.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the source byte offset where this text payload starts.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

/// Parsed text-table data from a `JILL1.VCL` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VclFile {
    /// Non-empty text entries preserved in table order.
    text_entries: Vec<VclTextEntry>,
}

impl VclFile {
    /// Parses `JILL1.VCL` text entries from a reader.
    ///
    /// Parsing semantics are intentionally OpenJill-compatible:
    /// - skip the 400-byte sound-entry region
    /// - read 40 `u32le` text offsets
    /// - read 40 `u16le` text lengths
    /// - materialize only non-empty text entries
    pub fn parse(reader: &mut ByteReader) -> Result<Self, VclReadError> {
        reader.skip(SOUND_ENTRY_SKIP).map_err(|source| VclReadError {
            field: "sound_entry_skip",
            entry_index: None,
            offset: error_offset(&source, 0),
            source,
        })?;

        let mut text_offsets = [0u32; TEXT_ENTRY_COUNT];
        let mut text_lengths = [0u16; TEXT_ENTRY_COUNT];

        for (entry_index, text_offset) in text_offsets.iter_mut().enumerate() {
            *text_offset = read_u32(reader, "text_offset", entry_index)?;
        }

        for (entry_index, text_length) in text_lengths.iter_mut().enumerate() {
            *text_length = read_u16(reader, "text_length", entry_index)?;
        }

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
                text,
                offset: text_offset,
            });
        }

        Ok(Self { text_entries })
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
}

/// Error returned when parsing a `JILL1.VCL` text table fails.
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
fn read_u8(reader: &mut ByteReader, field: &'static str, entry_index: usize) -> Result<u8, VclReadError> {
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
        ByteReaderError::UnexpectedEof { offset, .. } | ByteReaderError::OffsetOverflow { offset, .. } => {
            *offset
        }
        ByteReaderError::InvalidSeek { requested, .. } => *requested,
    }
    .max(lower_bound_offset)
}

#[cfg(test)]
mod tests {
    use super::{SOUND_ENTRY_SKIP, TEXT_ENTRY_COUNT, VclFile, VclReadError};
    use crate::ByteReaderError;
    use assert2::check;

    #[test]
    fn parses_sparse_text_entries_and_skips_empty_ones() {
        let mut bytes = vec![0; table_end()];

        write_text_entry(&mut bytes, 0, 700, 0);
        write_text_entry(&mut bytes, 5, 700, 5);
        write_text_entry(&mut bytes, 12, 705, 4);
        write_text_entry(&mut bytes, 39, 709, 0);

        write_text_at(&mut bytes, 700, b"HELLO");
        write_text_at(&mut bytes, 705, &[0x41, 0x00, 0xe9, 0x5a]);

        let vcl = VclFile::from_bytes(bytes).expect("VCL parse should succeed");

        check!(vcl.text_entry_count() == 2);
        check!(vcl.text_entries()[0].offset() == 700);
        check!(vcl.text_entries()[0].text() == "HELLO");
        check!(vcl.text_entries()[1].offset() == 705);
        check!(vcl.text_entries()[1].text() == "A\0éZ");
    }

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

    fn table_end() -> usize {
        SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (TEXT_ENTRY_COUNT * 2)
    }

    fn write_text_entry(bytes: &mut [u8], index: usize, offset: u32, length: u16) {
        let offset_pos = SOUND_ENTRY_SKIP + (index * 4);
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());

        let length_pos = SOUND_ENTRY_SKIP + (TEXT_ENTRY_COUNT * 4) + (index * 2);
        bytes[length_pos..length_pos + 2].copy_from_slice(&length.to_le_bytes());
    }

    fn write_text_at(bytes: &mut Vec<u8>, offset: usize, text: &[u8]) {
        let end = offset + text.len();
        if bytes.len() < end {
            bytes.resize(end, 0);
        }
        bytes[offset..end].copy_from_slice(text);
    }
}
