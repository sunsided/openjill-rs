//! Parser for `JILL.DMA` tile-metadata files.
//!
//! Each entry maps a 16-bit map code to its tile id, tileset, behaviour flags,
//! and human-readable name, mirroring the OpenJill data layout exactly.

use crate::{ByteReader, ByteReaderError};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Mask used to strip the flag bits from the tileset byte; the lower six bits
/// hold the tileset id and the upper two bits are reserved for flags.
const TILESET_MASK: u8 = 0x3f;
/// Flag bit indicating that the player can pass through the tile.
const FLAG_PLAYER_THRU: u16 = 0x01;
/// Flag bit set when the tile behaves as a stair.
const FLAG_STAIR: u16 = 0x02;
/// Flag bit set when the tile behaves as a vine (climbable).
const FLAG_VINE: u16 = 0x04;
/// Flag bit indicating that the tile reacts to the touch event.
const FLAG_MSG_TOUCH: u16 = 0x08;
/// Flag bit indicating that the tile reacts to the draw event.
const FLAG_MSG_DRAW: u16 = 0x10;
/// Flag bit indicating that the tile reacts to the update event.
const FLAG_MSG_UPDATE: u16 = 0x20;

/// One parsed entry from `JILL.DMA`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaEntry {
    /// 16-bit map code identifying the tile in level data.
    map_code: u16,
    /// Tile id within the resolved tileset.
    tile: u8,
    /// Tileset id (lower six bits of the on-disk tileset byte).
    tileset: u8,
    /// Behaviour flag bits parsed from the entry.
    flags: u16,
    /// Human-readable name preserved verbatim from the source bytes.
    name: String,
    /// Position of this entry within the parsed sequence.
    index: usize,
    /// Source byte offset where this entry starts in the original file.
    offset: usize,
}

impl DmaEntry {
    /// Returns the 16-bit map code.
    pub fn map_code(&self) -> u16 {
        self.map_code
    }

    /// Returns the tile id within the resolved tileset.
    pub fn tile(&self) -> u8 {
        self.tile
    }

    /// Returns the tileset id (lower six bits of the on-disk tileset byte).
    pub fn tileset(&self) -> u8 {
        self.tileset
    }

    /// Returns the raw flag bits parsed from the entry.
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Returns the entry's name as decoded from the source bytes.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the entry's position within the parsed sequence.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the source byte offset where this entry starts.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns `true` when the tile reacts to the touch event.
    pub fn is_msg_touch(&self) -> bool {
        (self.flags & FLAG_MSG_TOUCH) != 0
    }

    /// Returns `true` when the tile reacts to the draw event.
    pub fn is_msg_draw(&self) -> bool {
        (self.flags & FLAG_MSG_DRAW) != 0
    }

    /// Returns `true` when the tile reacts to the update event.
    pub fn is_msg_update(&self) -> bool {
        (self.flags & FLAG_MSG_UPDATE) != 0
    }

    /// Returns `true` when the player can pass through this tile.
    pub fn is_player_thru(&self) -> bool {
        (self.flags & FLAG_PLAYER_THRU) != 0
    }

    /// Returns `true` when this tile behaves as a stair (the `FLAG_STAIR` bit
    /// is set). Mirrors Java `BackgroundEntityImpl` `stair = (flag & F_STAIR)
    /// != 0`.
    pub fn is_stair(&self) -> bool {
        (self.flags & FLAG_STAIR) != 0
    }

    /// Returns `true` when this tile behaves as a vine / is climbable (the
    /// `FLAG_VINE` bit is set). Mirrors Java `BackgroundEntityImpl` `vine =
    /// (flag & F_VINE) != 0`; the bit-set tiles are the climbables (VNORM,
    /// POLET, VINEBR, …).
    pub fn is_vine(&self) -> bool {
        (self.flags & FLAG_VINE) != 0
    }
}

/// Parsed `JILL.DMA` tile-metadata file plus secondary lookup indices.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaFile {
    /// All parsed entries in source order.
    entries: Vec<DmaEntry>,
    /// Index of the most recently parsed entry for each map code.
    by_map_code: HashMap<u16, usize>,
    /// Index of the most recently parsed entry for each name.
    by_name: HashMap<String, usize>,
}

impl DmaFile {
    /// Parses a `JILL.DMA` file from the supplied reader.
    ///
    /// Consumes bytes sequentially until EOF. Lookup maps follow Java's
    /// `HashMap#put` semantics: when the same map code or name appears more
    /// than once, lookups resolve to the most recently parsed entry.
    pub fn parse(reader: &mut ByteReader) -> Result<Self, DmaReadError> {
        let mut entries = Vec::new();
        let mut by_map_code = HashMap::new();
        let mut by_name = HashMap::new();

        while reader.offset() < reader.len() {
            let entry_offset = reader.offset();

            let map_code = read_u16(reader, "map_code", entry_offset)?;
            let tile = read_u8(reader, "tile", entry_offset)?;
            let tileset_with_flags = read_u8(reader, "tileset_with_flags", entry_offset)?;
            let flags = read_u16(reader, "flags", entry_offset)?;
            let name_len = read_u8(reader, "name_len", entry_offset)? as usize;
            let name = read_name(reader, name_len, entry_offset)?;

            let entry = DmaEntry {
                map_code,
                tile,
                tileset: tileset_with_flags & TILESET_MASK,
                flags,
                name,
                index: entries.len(),
                offset: entry_offset,
            };

            // Keep Java semantics from HashMap#put: the most recent entry wins in lookups.
            by_map_code.insert(entry.map_code, entries.len());
            by_name.insert(entry.name.clone(), entries.len());
            entries.push(entry);
        }

        Ok(Self {
            entries,
            by_map_code,
            by_name,
        })
    }

    /// Parses a `DmaFile` directly from in-memory bytes.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, DmaReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader)
    }

    /// Returns all parsed entries in source order.
    pub fn entries(&self) -> &[DmaEntry] {
        &self.entries
    }

    /// Returns the number of parsed entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Looks up the entry associated with `map_code`, or `None` if no entry
    /// uses that code.
    pub fn get_by_map_code(&self, map_code: u16) -> Option<&DmaEntry> {
        self.by_map_code
            .get(&map_code)
            .map(|index| &self.entries[*index])
    }

    /// Looks up the entry associated with `name`, or `None` if no entry uses
    /// that name.
    pub fn get_by_name(&self, name: &str) -> Option<&DmaEntry> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }
}

/// Error returned when parsing a `JILL.DMA` file fails.
#[derive(Debug, Eq, PartialEq)]
pub struct DmaReadError {
    /// Name of the field being parsed when the failure occurred.
    pub field: &'static str,
    /// Source offset associated with the parse failure.
    pub offset: usize,
    /// Underlying byte-reader failure.
    source: ByteReaderError,
}

impl Display for DmaReadError {
    /// Formats the error including the field name, offset, and underlying cause.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "failed to parse DMA field '{}' at offset {}: {}",
            self.field, self.offset, self.source
        )
    }
}

impl Error for DmaReadError {
    /// Returns the underlying byte-reader error as the cause of this failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Reads a single `u8` field and wraps reader errors with DMA parse context.
fn read_u8(
    reader: &mut ByteReader,
    field: &'static str,
    entry_offset: usize,
) -> Result<u8, DmaReadError> {
    let offset = reader.offset();
    reader.read_u8().map_err(|source| DmaReadError {
        field,
        offset: error_offset(&source, offset, entry_offset),
        source,
    })
}

/// Reads a single `u16le` field and wraps reader errors with DMA parse context.
fn read_u16(
    reader: &mut ByteReader,
    field: &'static str,
    entry_offset: usize,
) -> Result<u16, DmaReadError> {
    let offset = reader.offset();
    reader.read_u16_le().map_err(|source| DmaReadError {
        field,
        offset: error_offset(&source, offset, entry_offset),
        source,
    })
}

/// Reads `name_len` bytes as a length-prefixed name string, preserving each
/// source byte as a single `U+0000..U+00FF` code point (matches Java's
/// `(char) read8bitLE()` behaviour).
fn read_name(
    reader: &mut ByteReader,
    name_len: usize,
    entry_offset: usize,
) -> Result<String, DmaReadError> {
    let mut name = String::with_capacity(name_len);

    for _ in 0..name_len {
        let byte = read_u8(reader, "name", entry_offset)?;
        // Keep parser compatibility with Java's `(char) read8bitLE()`: each source byte
        // is preserved as a single code point in the U+0000..U+00FF range.
        name.push(char::from(byte));
    }

    Ok(name)
}

/// Chooses the most useful parse-failure offset to report for a reader error.
///
/// Falls back to the per-field offset for invalid seeks when it points past
/// the entry start; otherwise reports the start of the failing entry.
fn error_offset(source: &ByteReaderError, fallback: usize, entry_offset: usize) -> usize {
    match source {
        ByteReaderError::UnexpectedEof { offset, .. }
        | ByteReaderError::OffsetOverflow { offset, .. } => *offset,
        ByteReaderError::InvalidSeek { .. } => {
            if fallback >= entry_offset {
                fallback
            } else {
                entry_offset
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DmaFile, DmaReadError};
    use crate::ByteReaderError;
    use assert2::check;

    /// Unit under test: `DmaFile::parse`.
    ///
    /// Preconditions: a synthetic three-entry buffer is built with distinct
    /// map codes, names, and tileset/flag values.
    ///
    /// Invariants asserted: parsed entries preserve their order, indices, and
    /// computed source byte offsets; map-code and name lookups resolve back to
    /// the original entries; missing keys produce `None`.
    #[test]
    fn parses_entries_offsets_indexes_names_and_lookups() {
        let bytes = dma_bytes(&[
            (0x1234, 0x08, 0x01, 0x0000, "STONE"),
            (0x2345, 0x0a, 0x3e, 0x0009, "VINE"),
            (0x3456, 0x0b, 0x7f, 0x0002, "LADDER"),
        ]);

        let dma = DmaFile::from_bytes(bytes).expect("DMA parse should succeed");
        check!(dma.entry_count() == 3);

        let first = &dma.entries()[0];
        check!(first.index() == 0);
        check!(first.offset() == 0);
        check!(first.name() == "STONE");

        let second = &dma.entries()[1];
        check!(second.index() == 1);
        check!(second.offset() == 12);
        check!(second.name() == "VINE");

        check!(
            let Some(third) = dma.get_by_map_code(0x3456)
                && third.index() == 2
                && third.offset() == 23
                && third.name() == "LADDER"
        );

        check!(let Some(by_name) = dma.get_by_name("VINE") && by_name.map_code() == 0x2345);
        check!(dma.get_by_map_code(0xffff).is_none());
        check!(dma.get_by_name("missing").is_none());
    }

    /// Unit under test: tileset masking inside `DmaFile::parse`.
    ///
    /// Preconditions: a single entry with the tileset-with-flags byte set to
    /// `0xff`, exercising every bit of the upper-flag region.
    ///
    /// Invariants asserted: only the lower six bits survive into
    /// `DmaEntry::tileset`, regardless of the upper bits set on disk.
    #[test]
    fn masks_tileset_value_to_lower_six_bits() {
        let dma = DmaFile::from_bytes(dma_bytes(&[(0x1000, 0x10, 0xff, 0, "MASK")]))
            .expect("DMA parse should succeed");
        check!(dma.entries()[0].tileset() == 0x3f);
    }

    /// Unit under test: the flag-helper accessors on `DmaEntry`.
    ///
    /// Preconditions: two synthetic entries — one with every flag bit set
    /// (`0x3f`), one with no flag bits set (`0x00`).
    ///
    /// Invariants asserted: every helper resolves the corresponding flag
    /// (`is_msg_*`, `is_player_thru`, `is_stair`, `is_vine`) with the Java
    /// `BackgroundEntityImpl` semantics — a set bit means the behaviour is
    /// enabled (stair/vine are climbable when their bit is set).
    #[test]
    fn preserves_flag_helper_semantics() {
        let dma = DmaFile::from_bytes(dma_bytes(&[(1, 1, 1, 0x3f, "A"), (2, 2, 2, 0x00, "B")]))
            .expect("DMA parse should succeed");

        let all_flags = &dma.entries()[0];
        check!(all_flags.is_msg_touch());
        check!(all_flags.is_msg_draw());
        check!(all_flags.is_msg_update());
        check!(all_flags.is_player_thru());
        check!(all_flags.is_stair());
        check!(all_flags.is_vine());

        let no_flags = &dma.entries()[1];
        check!(!no_flags.is_msg_touch());
        check!(!no_flags.is_msg_draw());
        check!(!no_flags.is_msg_update());
        check!(!no_flags.is_player_thru());
        check!(!no_flags.is_stair());
        check!(!no_flags.is_vine());
    }

    /// Unit under test: `DmaReadError` reporting in `DmaFile::parse`.
    ///
    /// Preconditions: a four-byte input that completes the `map_code`, `tile`,
    /// and `tileset_with_flags` fields but truncates before the `flags` field.
    ///
    /// Invariants asserted: parsing fails with a `DmaReadError` whose field is
    /// `"flags"` and whose offset matches the EOF position reported by the
    /// underlying `ByteReaderError`.
    #[test]
    fn includes_failing_offset_in_errors() {
        check!(
            let Err(err) = DmaFile::from_bytes([0x34, 0x12, 0x7f, 0x01])
                && err == DmaReadError {
                    field: "flags",
                    offset: 4,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read unsigned 16-bit little-endian integer",
                        offset: 4,
                        requested: 2,
                        len: 4,
                    },
                }
        );
    }

    /// Unit under test: last-wins semantics of the lookup maps populated by
    /// `DmaFile::parse`.
    ///
    /// Preconditions: three entries that deliberately collide on map codes
    /// (`0x1000` repeated) and names (`"DUP"` repeated).
    ///
    /// Invariants asserted: `entries()` keeps every entry in input order,
    /// while `get_by_map_code` and `get_by_name` resolve to the most recently
    /// parsed entry for the duplicated key.
    #[test]
    fn lookup_maps_follow_last_wins_semantics_for_duplicates() {
        let dma = DmaFile::from_bytes(dma_bytes(&[
            (0x1000, 1, 1, 0, "DUP"),
            (0x1000, 2, 2, 0, "SECOND"),
            (0x2000, 3, 3, 0, "DUP"),
        ]))
        .expect("DMA parse should succeed");

        check!(dma.entries()[0].tile() == 1);
        check!(dma.entries()[1].tile() == 2);
        check!(dma.entries()[2].tile() == 3);

        check!(let Some(last_map) = dma.get_by_map_code(0x1000) && last_map.name() == "SECOND");
        check!(let Some(last_name) = dma.get_by_name("DUP") && last_name.map_code() == 0x2000);
    }

    /// Builds a synthetic on-disk `JILL.DMA` byte buffer from a list of
    /// `(map_code, tile, tileset_with_flags, flags, name)` tuples, mirroring
    /// the on-disk layout exactly so unit tests can drive `DmaFile::parse`
    /// without real game data.
    fn dma_bytes(entries: &[(u16, u8, u8, u16, &str)]) -> Vec<u8> {
        let mut bytes = Vec::new();

        for (map_code, tile, tileset_with_flags, flags, name) in entries {
            bytes.extend(map_code.to_le_bytes());
            bytes.push(*tile);
            bytes.push(*tileset_with_flags);
            bytes.extend(flags.to_le_bytes());
            bytes.push(u8::try_from(name.len()).expect("name length must fit in u8"));
            bytes.extend(name.bytes());
        }

        bytes
    }
}
