use crate::{ByteReader, ByteReaderError};
use std::collections::HashMap;
use std::error::Error;
use std::fmt::{Display, Formatter};

const TILESET_MASK: u8 = 0x3f;
const FLAG_PLAYER_THRU: u16 = 0x01;
const FLAG_NOT_STAIR: u16 = 0x02;
const FLAG_NOT_VINE: u16 = 0x04;
const FLAG_MSG_TOUCH: u16 = 0x08;
const FLAG_MSG_DRAW: u16 = 0x10;
const FLAG_MSG_UPDATE: u16 = 0x20;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaEntry {
    map_code: u16,
    tile: u8,
    tileset: u8,
    flags: u16,
    name: String,
    index: usize,
    offset: usize,
}

impl DmaEntry {
    pub fn map_code(&self) -> u16 {
        self.map_code
    }

    pub fn tile(&self) -> u8 {
        self.tile
    }

    pub fn tileset(&self) -> u8 {
        self.tileset
    }

    pub fn flags(&self) -> u16 {
        self.flags
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn is_msg_touch(&self) -> bool {
        (self.flags & FLAG_MSG_TOUCH) != 0
    }

    pub fn is_msg_draw(&self) -> bool {
        (self.flags & FLAG_MSG_DRAW) != 0
    }

    pub fn is_msg_update(&self) -> bool {
        (self.flags & FLAG_MSG_UPDATE) != 0
    }

    pub fn is_player_thru(&self) -> bool {
        (self.flags & FLAG_PLAYER_THRU) != 0
    }

    pub fn is_stair(&self) -> bool {
        (self.flags & FLAG_NOT_STAIR) == 0
    }

    pub fn is_vine(&self) -> bool {
        (self.flags & FLAG_NOT_VINE) == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DmaFile {
    entries: Vec<DmaEntry>,
    by_map_code: HashMap<u16, usize>,
    by_name: HashMap<String, usize>,
}

impl DmaFile {
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

    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, DmaReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader)
    }

    pub fn entries(&self) -> &[DmaEntry] {
        &self.entries
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn get_by_map_code(&self, map_code: u16) -> Option<&DmaEntry> {
        self.by_map_code
            .get(&map_code)
            .map(|index| &self.entries[*index])
    }

    pub fn get_by_name(&self, name: &str) -> Option<&DmaEntry> {
        self.by_name.get(name).map(|index| &self.entries[*index])
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct DmaReadError {
    pub field: &'static str,
    pub offset: usize,
    source: ByteReaderError,
}

impl Display for DmaReadError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "failed to parse DMA field '{}' at offset {}: {}",
            self.field, self.offset, self.source
        )
    }
}

impl Error for DmaReadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

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

    #[test]
    fn masks_tileset_value_to_lower_six_bits() {
        let dma = DmaFile::from_bytes(dma_bytes(&[(0x1000, 0x10, 0xff, 0, "MASK")]))
            .expect("DMA parse should succeed");
        check!(dma.entries()[0].tileset() == 0x3f);
    }

    #[test]
    fn preserves_flag_helper_semantics() {
        let dma = DmaFile::from_bytes(dma_bytes(&[(1, 1, 1, 0x39, "A"), (2, 2, 2, 0x06, "B")]))
            .expect("DMA parse should succeed");

        let all_messages = &dma.entries()[0];
        check!(all_messages.is_msg_touch());
        check!(all_messages.is_msg_draw());
        check!(all_messages.is_msg_update());
        check!(all_messages.is_player_thru());
        check!(all_messages.is_stair());
        check!(all_messages.is_vine());

        let no_stair_no_vine = &dma.entries()[1];
        check!(!no_stair_no_vine.is_msg_touch());
        check!(!no_stair_no_vine.is_msg_draw());
        check!(!no_stair_no_vine.is_msg_update());
        check!(!no_stair_no_vine.is_player_thru());
        check!(!no_stair_no_vine.is_stair());
        check!(!no_stair_no_vine.is_vine());
    }

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
