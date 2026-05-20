//! Parser for `JILL1.SHA` shape/tile/font/picture data.
//!
//! The parser preserves the indexed image bytes and optional per-tileset color
//! maps so later renderer stages can perform video-mode-specific expansion
//! without this crate committing to RGBA output.

use crate::{ByteReader, ByteReaderError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Number of tileset entries stored in the SHA file header.
const TILESET_ENTRY_COUNT: usize = 128;
/// Bit flag indicating that a tileset contains font glyphs.
const FLAG_FONT: u16 = 0x0001;
/// Bit flag indicating that a tileset contains level tiles.
const FLAG_LEVEL_TILESET: u16 = 0x0004;
/// One header entry from the `JILL1.SHA` tileset table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaHeaderEntry {
    /// Position of this entry inside the 128-entry header table.
    index: usize,
    /// Byte offset of the tileset data within the SHA file.
    offset: u32,
    /// Declared byte size of the tileset data within the SHA file.
    size: u16,
}

impl ShaHeaderEntry {
    /// Returns the entry's position inside the 128-entry header table.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the tileset byte offset preserved from the file header.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the tileset byte size preserved from the file header.
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Returns `true` when this entry is considered valid by OpenJill's
    /// original `(offset != 0 && size != 0)` rule.
    pub fn is_valid(&self) -> bool {
        self.offset != 0 && self.size != 0
    }
}

/// Parsed 128-entry header table from a `JILL1.SHA` file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaHeader {
    /// All header entries in their original source order.
    entries: Vec<ShaHeaderEntry>,
}

impl ShaHeader {
    /// Returns all 128 header entries in source order.
    pub fn entries(&self) -> &[ShaHeaderEntry] {
        &self.entries
    }

    /// Returns the header entry at `index`, or `None` when the index lies
    /// outside the fixed 128-entry table.
    pub fn entry(&self, index: usize) -> Option<&ShaHeaderEntry> {
        self.entries.get(index)
    }

    /// Returns the number of valid `(offset != 0 && size != 0)` entries.
    pub fn valid_entry_count(&self) -> usize {
        self.entries.iter().filter(|entry| entry.is_valid()).count()
    }
}

/// One preserved 4-byte color-map entry shared by every tile in a tileset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaColorMapEntry {
    /// CGA palette index for this logical color.
    cga: u8,
    /// EGA palette index for this logical color.
    ega: u8,
    /// VGA palette index for this logical color.
    vga: u8,
    /// Reserved byte preserved verbatim from disk.
    reserved: u8,
}

impl ShaColorMapEntry {
    /// Creates a color-map entry from explicit component bytes.
    pub fn new(cga: u8, ega: u8, vga: u8, reserved: u8) -> Self {
        Self {
            cga,
            ega,
            vga,
            reserved,
        }
    }

    /// Returns the preserved CGA palette index.
    pub fn cga(&self) -> u8 {
        self.cga
    }

    /// Returns the preserved EGA palette index.
    pub fn ega(&self) -> u8 {
        self.ega
    }

    /// Returns the preserved VGA palette index.
    pub fn vga(&self) -> u8 {
        self.vga
    }

    /// Returns the reserved trailing byte preserved from disk.
    pub fn reserved(&self) -> u8 {
        self.reserved
    }
}

/// One indexed image record from a SHA tileset.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaTile {
    /// Header entry index of the parent tileset.
    tileset_index: usize,
    /// Position of this tile within its parent tileset.
    image_index: usize,
    /// Width preserved from the on-disk image header.
    width: u8,
    /// Height preserved from the on-disk image header.
    height: u8,
    /// Data-format byte preserved verbatim from disk.
    data_format: u8,
    /// Source byte offset where indexed pixel data begins.
    offset: usize,
    /// Row-major indexed pixels preserved verbatim from disk.
    indexed_pixels: Vec<u8>,
}

impl ShaTile {
    /// Returns the parent tileset's header entry index.
    pub fn tileset_index(&self) -> usize {
        self.tileset_index
    }

    /// Returns this tile's position within its parent tileset.
    pub fn image_index(&self) -> usize {
        self.image_index
    }

    /// Returns the tile width preserved from the image header.
    pub fn width(&self) -> u8 {
        self.width
    }

    /// Returns the tile height preserved from the image header.
    pub fn height(&self) -> u8 {
        self.height
    }

    /// Returns the data-format byte preserved from the image header.
    pub fn data_format(&self) -> u8 {
        self.data_format
    }

    /// Returns the source byte offset where indexed pixels begin.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the row-major indexed pixels preserved from disk.
    pub fn indexed_pixels(&self) -> &[u8] {
        &self.indexed_pixels
    }
}

/// One parsed tileset from `JILL1.SHA`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaTileSet {
    /// Header entry index associated with this tileset.
    entry_index: usize,
    /// Byte offset preserved from the SHA header.
    offset: u32,
    /// Byte size preserved from the SHA header.
    size: u16,
    /// Number of tile/image records declared by the tileset header.
    tile_count: u8,
    /// Rotation field preserved from the tileset header.
    rotations: u16,
    /// CGA decompressed-size field preserved from the tileset header.
    cga_size: u16,
    /// EGA decompressed-size field preserved from the tileset header.
    ega_size: u16,
    /// VGA decompressed-size field preserved from the tileset header.
    vga_size: u16,
    /// Bit depth preserved from the tileset header.
    bit_depth: u8,
    /// Raw flags preserved from the tileset header.
    flags: u16,
    /// Optional shared color map for indexed tilesets below 8 bits.
    color_map: Option<Vec<ShaColorMapEntry>>,
    /// All parsed tile/image records in source order.
    tiles: Vec<ShaTile>,
}

impl ShaTileSet {
    /// Returns the header entry index associated with this tileset.
    pub fn entry_index(&self) -> usize {
        self.entry_index
    }

    /// Returns the tileset byte offset preserved from the SHA header.
    pub fn offset(&self) -> u32 {
        self.offset
    }

    /// Returns the tileset byte size preserved from the SHA header.
    pub fn size(&self) -> u16 {
        self.size
    }

    /// Returns the number of tile/image records declared by the tileset header.
    pub fn tile_count(&self) -> u8 {
        self.tile_count
    }

    /// Returns the rotations field preserved from the tileset header.
    pub fn rotations(&self) -> u16 {
        self.rotations
    }

    /// Returns the CGA decompressed-size field.
    pub fn cga_size(&self) -> u16 {
        self.cga_size
    }

    /// Returns the EGA decompressed-size field.
    pub fn ega_size(&self) -> u16 {
        self.ega_size
    }

    /// Returns the VGA decompressed-size field.
    pub fn vga_size(&self) -> u16 {
        self.vga_size
    }

    /// Returns the bit depth preserved from the tileset header.
    pub fn bit_depth(&self) -> u8 {
        self.bit_depth
    }

    /// Returns the raw tileset flags preserved from disk.
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Returns `true` when the `SHM_FONTF` flag is set.
    pub fn is_font(&self) -> bool {
        (self.flags & FLAG_FONT) != 0
    }

    /// Returns `true` when the `SHM_BLFLAG` flag is set.
    pub fn is_level_tileset(&self) -> bool {
        (self.flags & FLAG_LEVEL_TILESET) != 0
    }

    /// Returns the optional shared color map preserved from disk.
    pub fn color_map(&self) -> Option<&[ShaColorMapEntry]> {
        self.color_map.as_deref()
    }

    /// Returns all parsed tile/image records in source order.
    pub fn tiles(&self) -> &[ShaTile] {
        &self.tiles
    }
}

/// Parsed `JILL1.SHA` file consisting of a header plus valid tilesets.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaFile {
    /// Parsed 128-entry SHA header.
    header: ShaHeader,
    /// All valid tilesets referenced by the header, in header order.
    tilesets: Vec<ShaTileSet>,
}

impl ShaFile {
    /// Parses a `JILL1.SHA` file from the supplied reader.
    pub fn parse(reader: &mut ByteReader) -> Result<Self, ShaReadError> {
        let header = parse_header(reader)?;
        let mut tilesets = Vec::with_capacity(header.valid_entry_count());

        for entry in header.entries().iter().filter(|entry| entry.is_valid()) {
            tilesets.push(parse_tileset(reader, entry)?);
        }

        Ok(Self { header, tilesets })
    }

    /// Parses a `ShaFile` directly from in-memory bytes.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, ShaReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader)
    }

    /// Returns the parsed 128-entry SHA header.
    pub fn header(&self) -> &ShaHeader {
        &self.header
    }

    /// Returns all valid tilesets referenced by the header.
    pub fn tilesets(&self) -> &[ShaTileSet] {
        &self.tilesets
    }
}

/// Error returned when parsing `JILL1.SHA` fails.
#[derive(Debug, Eq, PartialEq)]
pub struct ShaReadError {
    /// Name of the field being parsed when the failure occurred.
    pub field: &'static str,
    /// Optional header entry index associated with the failing read.
    pub entry_index: Option<usize>,
    /// Optional tile index associated with the failing read.
    pub tile_index: Option<usize>,
    /// Source offset associated with the parse failure.
    pub offset: usize,
    /// Underlying byte-reader failure.
    source: ByteReaderError,
}

impl Display for ShaReadError {
    /// Formats the error including field name, parse context, and byte offset.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.entry_index, self.tile_index) {
            (Some(entry_index), Some(tile_index)) => write!(
                f,
                "failed to parse SHA field '{}' for entry {} tile {} at offset {}: {}",
                self.field, entry_index, tile_index, self.offset, self.source
            ),
            (Some(entry_index), None) => write!(
                f,
                "failed to parse SHA field '{}' for entry {} at offset {}: {}",
                self.field, entry_index, self.offset, self.source
            ),
            (None, Some(tile_index)) => write!(
                f,
                "failed to parse SHA field '{}' for tile {} at offset {}: {}",
                self.field, tile_index, self.offset, self.source
            ),
            (None, None) => write!(
                f,
                "failed to parse SHA field '{}' at offset {}: {}",
                self.field, self.offset, self.source
            ),
        }
    }
}

impl Error for ShaReadError {
    /// Returns the underlying byte-reader error as the cause of this failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Parses the fixed-size 128-entry SHA header tables.
fn parse_header(reader: &mut ByteReader) -> Result<ShaHeader, ShaReadError> {
    let mut offsets = [0u32; TILESET_ENTRY_COUNT];
    let mut sizes = [0u16; TILESET_ENTRY_COUNT];

    for (index, offset) in offsets.iter_mut().enumerate() {
        *offset = read_u32(reader, "tileset_offset", Some(index), None)?;
    }

    for (index, size) in sizes.iter_mut().enumerate() {
        *size = read_u16(reader, "tileset_size", Some(index), None)?;
    }

    let mut entries = Vec::with_capacity(TILESET_ENTRY_COUNT);
    for index in 0..TILESET_ENTRY_COUNT {
        entries.push(ShaHeaderEntry {
            index,
            offset: offsets[index],
            size: sizes[index],
        });
    }

    Ok(ShaHeader { entries })
}

/// Parses one valid tileset referenced by the SHA header.
fn parse_tileset(
    reader: &mut ByteReader,
    entry: &ShaHeaderEntry,
) -> Result<ShaTileSet, ShaReadError> {
    let seek_offset = usize::try_from(entry.offset()).expect("u32 offsets must fit in usize");
    seek(
        reader,
        "tileset_offset",
        Some(entry.index()),
        None,
        seek_offset,
    )?;

    let tile_count = read_u8(reader, "tile_count", Some(entry.index()), None)?;
    let rotations = read_u16(reader, "rotations", Some(entry.index()), None)?;
    let cga_size = read_u16(reader, "cga_size", Some(entry.index()), None)?;
    let ega_size = read_u16(reader, "ega_size", Some(entry.index()), None)?;
    let vga_size = read_u16(reader, "vga_size", Some(entry.index()), None)?;
    let bit_depth = read_u8(reader, "bit_depth", Some(entry.index()), None)?;
    let flags = read_u16(reader, "flags", Some(entry.index()), None)?;

    let is_font = (flags & FLAG_FONT) != 0;
    let color_map = if !is_font && bit_depth < 8 {
        Some(parse_color_map(reader, entry.index(), bit_depth)?)
    } else {
        None
    };

    let mut tiles = Vec::with_capacity(usize::from(tile_count));
    for tile_index in 0..usize::from(tile_count) {
        tiles.push(parse_tile(reader, entry.index(), tile_index)?);
    }

    Ok(ShaTileSet {
        entry_index: entry.index(),
        offset: entry.offset(),
        size: entry.size(),
        tile_count,
        rotations,
        cga_size,
        ega_size,
        vga_size,
        bit_depth,
        flags,
        color_map,
        tiles,
    })
}

/// Parses the shared optional color map for a non-font tileset below 8 bits.
fn parse_color_map(
    reader: &mut ByteReader,
    entry_index: usize,
    bit_depth: u8,
) -> Result<Vec<ShaColorMapEntry>, ShaReadError> {
    let entry_count = 1usize << bit_depth;
    let mut color_map = Vec::with_capacity(entry_count);

    for _ in 0..entry_count {
        color_map.push(ShaColorMapEntry {
            cga: read_u8(reader, "color_map_cga", Some(entry_index), None)?,
            ega: read_u8(reader, "color_map_ega", Some(entry_index), None)?,
            vga: read_u8(reader, "color_map_vga", Some(entry_index), None)?,
            reserved: read_u8(reader, "color_map_reserved", Some(entry_index), None)?,
        });
    }

    Ok(color_map)
}

/// Parses one tile/image record from a SHA tileset.
fn parse_tile(
    reader: &mut ByteReader,
    entry_index: usize,
    tile_index: usize,
) -> Result<ShaTile, ShaReadError> {
    let width = read_u8(reader, "width", Some(entry_index), Some(tile_index))?;
    let height = read_u8(reader, "height", Some(entry_index), Some(tile_index))?;
    let data_format = read_u8(reader, "data_format", Some(entry_index), Some(tile_index))?;
    let offset = reader.offset();
    let pixel_count = usize::from(width) * usize::from(height);
    let mut indexed_pixels = Vec::with_capacity(pixel_count);

    for _ in 0..pixel_count {
        indexed_pixels.push(read_u8(
            reader,
            "indexed_pixel",
            Some(entry_index),
            Some(tile_index),
        )?);
    }

    Ok(ShaTile {
        tileset_index: entry_index,
        image_index: tile_index,
        width,
        height,
        data_format,
        offset,
        indexed_pixels,
    })
}

/// Reads a single `u8` field and wraps reader errors with SHA parse context.
fn read_u8(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
    tile_index: Option<usize>,
) -> Result<u8, ShaReadError> {
    let fallback_offset = reader.offset();
    reader
        .read_u8()
        .map_err(|source| sha_read_error(field, entry_index, tile_index, fallback_offset, source))
}

/// Reads a single `u16le` field and wraps reader errors with SHA parse context.
fn read_u16(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
    tile_index: Option<usize>,
) -> Result<u16, ShaReadError> {
    let fallback_offset = reader.offset();
    reader
        .read_u16_le()
        .map_err(|source| sha_read_error(field, entry_index, tile_index, fallback_offset, source))
}

/// Reads a single `u32le` field and wraps reader errors with SHA parse context.
fn read_u32(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
    tile_index: Option<usize>,
) -> Result<u32, ShaReadError> {
    let fallback_offset = reader.offset();
    reader
        .read_u32_le()
        .map_err(|source| sha_read_error(field, entry_index, tile_index, fallback_offset, source))
}

/// Seeks to `offset` and wraps seek failures with SHA parse context.
fn seek(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
    tile_index: Option<usize>,
    offset: usize,
) -> Result<(), ShaReadError> {
    reader
        .seek(offset)
        .map_err(|source| sha_read_error(field, entry_index, tile_index, offset, source))
}

/// Builds a contextual `ShaReadError` from a lower-level byte-reader failure.
fn sha_read_error(
    field: &'static str,
    entry_index: Option<usize>,
    tile_index: Option<usize>,
    fallback_offset: usize,
    source: ByteReaderError,
) -> ShaReadError {
    ShaReadError {
        field,
        entry_index,
        tile_index,
        offset: error_offset(&source, fallback_offset),
        source,
    }
}

/// Chooses the most useful byte offset to report for a reader error.
fn error_offset(source: &ByteReaderError, fallback_offset: usize) -> usize {
    match source {
        ByteReaderError::UnexpectedEof { offset, .. }
        | ByteReaderError::OffsetOverflow { offset, .. } => *offset,
        ByteReaderError::InvalidSeek { requested, .. } => *requested,
    }
    .max(fallback_offset)
}

/// Unit tests covering synthetic SHA fixtures and error reporting.
#[cfg(test)]
mod tests {
    use super::{FLAG_FONT, FLAG_LEVEL_TILESET, ShaFile, ShaReadError, TILESET_ENTRY_COUNT};
    use crate::ByteReaderError;
    use assert2::check;

    /// Byte length of the fixed-size SHA header (`128 * u32le` offsets followed
    /// by `128 * u16le` sizes).
    const HEADER_LEN: usize = (TILESET_ENTRY_COUNT * 4) + (TILESET_ENTRY_COUNT * 2);
    /// Number of bytes in one synthetic color-map entry.
    const COLOR_MAP_ENTRY_LEN: usize = 4;

    /// Synthetic SHA fixture plus the expected offsets/sizes needed by the
    /// assertions in the parser unit tests.
    struct SyntheticShaFixture {
        /// Complete synthetic SHA byte buffer.
        bytes: Vec<u8>,
        /// Expected offset of the first valid tileset.
        level_tileset_offset: u32,
        /// Expected size of the first valid tileset.
        level_tileset_size: u16,
        /// Expected pixel-data offsets for the first valid tileset's tiles.
        level_tile_offsets: [usize; 2],
        /// Expected offset of the second valid tileset.
        font_tileset_offset: u32,
        /// Expected size of the second valid tileset.
        font_tileset_size: u16,
        /// Expected pixel-data offset for the second valid tileset's only tile.
        font_tile_offset: usize,
    }

    /// Unit under test: `ShaFile::parse`.
    ///
    /// Preconditions: a hand-built SHA file contains four interesting header
    /// entries — one valid level-tileset entry with a 2-bit shared color map
    /// and two tiles, one valid font entry with `bit_depth == 1` but no color
    /// map, one invalid nonzero-offset/zero-size entry, and one invalid
    /// zero-offset/nonzero-size entry.
    ///
    /// Invariants asserted: header validity follows the original
    /// `(offset != 0 && size != 0)` rule, only valid entries materialize into
    /// parsed tilesets, metadata fields and font/level-tileset flag semantics
    /// are preserved, the non-font low-bit-depth tileset retains its color-map
    /// entries verbatim, the font tileset omits a color map despite
    /// `bit_depth < 8`, and every parsed tile preserves dimensions, data
    /// format, source pixel offset, and row-major indexed pixel data.
    #[test]
    fn parses_header_entries_tileset_metadata_color_maps_and_tiles() {
        let fixture = build_synthetic_sha_fixture();

        let sha = ShaFile::from_bytes(fixture.bytes).expect("SHA parse should succeed");

        check!(sha.header().entries().len() == TILESET_ENTRY_COUNT);
        check!(sha.header().valid_entry_count() == 2);
        check!(let Some(entry) = sha.header().entry(0) && !entry.is_valid());
        check!(
            let Some(entry) = sha.header().entry(1)
                && entry.is_valid()
                && entry.offset() == fixture.level_tileset_offset
                && entry.size() == fixture.level_tileset_size
        );
        check!(let Some(entry) = sha.header().entry(3) && !entry.is_valid());
        check!(let Some(entry) = sha.header().entry(4) && !entry.is_valid());

        check!(sha.tilesets().len() == 2);

        let level = &sha.tilesets()[0];
        check!(level.entry_index() == 1);
        check!(level.offset() == fixture.level_tileset_offset);
        check!(level.size() == fixture.level_tileset_size);
        check!(level.tile_count() == 2);
        check!(level.rotations() == 3);
        check!(level.cga_size() == 11);
        check!(level.ega_size() == 12);
        check!(level.vga_size() == 13);
        check!(level.bit_depth() == 2);
        check!(level.flags() == FLAG_LEVEL_TILESET);
        check!(!level.is_font());
        check!(level.is_level_tileset());
        check!(let Some(color_map) = level.color_map() && color_map.len() == 4);

        let level_color_map = level
            .color_map()
            .expect("level tileset should have color map");
        check!(level_color_map[0].cga() == 1);
        check!(level_color_map[0].ega() == 10);
        check!(level_color_map[0].vga() == 20);
        check!(level_color_map[0].reserved() == 0);
        check!(level_color_map[3].cga() == 4);
        check!(level_color_map[3].ega() == 13);
        check!(level_color_map[3].vga() == 23);
        check!(level_color_map[3].reserved() == 0);

        check!(level.tiles().len() == 2);
        let first_level_tile = &level.tiles()[0];
        check!(first_level_tile.tileset_index() == 1);
        check!(first_level_tile.image_index() == 0);
        check!(first_level_tile.width() == 2);
        check!(first_level_tile.height() == 2);
        check!(first_level_tile.data_format() == 7);
        check!(first_level_tile.offset() == fixture.level_tile_offsets[0]);
        check!(first_level_tile.indexed_pixels() == [0, 1, 2, 3]);

        let second_level_tile = &level.tiles()[1];
        check!(second_level_tile.tileset_index() == 1);
        check!(second_level_tile.image_index() == 1);
        check!(second_level_tile.width() == 1);
        check!(second_level_tile.height() == 3);
        check!(second_level_tile.data_format() == 9);
        check!(second_level_tile.offset() == fixture.level_tile_offsets[1]);
        check!(second_level_tile.indexed_pixels() == [3, 2, 1]);

        let font = &sha.tilesets()[1];
        check!(font.entry_index() == 2);
        check!(font.offset() == fixture.font_tileset_offset);
        check!(font.size() == fixture.font_tileset_size);
        check!(font.tile_count() == 1);
        check!(font.rotations() == 1);
        check!(font.cga_size() == 21);
        check!(font.ega_size() == 22);
        check!(font.vga_size() == 23);
        check!(font.bit_depth() == 1);
        check!(font.flags() == FLAG_FONT);
        check!(font.is_font());
        check!(!font.is_level_tileset());
        check!(font.color_map().is_none());
        check!(font.tiles().len() == 1);
        check!(font.tiles()[0].offset() == fixture.font_tile_offset);
        check!(font.tiles()[0].indexed_pixels() == [9, 8]);
    }

    /// Unit under test: `ShaReadError` reporting while parsing the fixed-size
    /// header tables.
    ///
    /// Preconditions: the synthetic input is truncated one byte before the end
    /// of the size table, so entry 127 starts but cannot complete its `u16le`
    /// read.
    ///
    /// Invariants asserted: parsing fails with `field == "tileset_size"`,
    /// `entry_index == Some(127)`, no tile context, and the reported offset
    /// equals the start of the failing size read.
    #[test]
    fn reports_failing_offset_for_truncated_header_size_table() {
        let bytes = vec![0; HEADER_LEN - 1];

        check!(
            let Err(err) = ShaFile::from_bytes(bytes)
                && err == ShaReadError {
                    field: "tileset_size",
                    entry_index: Some(127),
                    tile_index: None,
                    offset: HEADER_LEN - 2,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read unsigned 16-bit little-endian integer",
                        offset: HEADER_LEN - 2,
                        requested: 2,
                        len: HEADER_LEN - 1,
                    },
                }
        );
    }

    /// Unit under test: `ShaReadError` reporting while reading indexed pixel
    /// data for a tile record.
    ///
    /// Preconditions: a single valid tileset is declared with one `2x2` tile,
    /// but only three indexed pixels are present in the file even though the
    /// tile header requires four.
    ///
    /// Invariants asserted: parsing fails with `field == "indexed_pixel"`,
    /// the entry/tile indices point at the only tileset and tile, and the
    /// reported offset equals the missing fourth pixel's source position.
    #[test]
    fn reports_failing_offset_for_truncated_tile_pixels() {
        let bytes = truncated_tile_pixels_fixture();

        check!(
            let Err(err) = ShaFile::from_bytes(bytes)
                && err == ShaReadError {
                    field: "indexed_pixel",
                    entry_index: Some(1),
                    tile_index: Some(0),
                    offset: HEADER_LEN + 18,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read unsigned 8-bit integer",
                        offset: HEADER_LEN + 18,
                        requested: 1,
                        len: HEADER_LEN + 18,
                    },
                }
        );
    }

    /// Builds a synthetic SHA fixture containing one valid level tileset, one
    /// valid font tileset, and two invalid header entries for validity tests.
    fn build_synthetic_sha_fixture() -> SyntheticShaFixture {
        let mut bytes = vec![0; HEADER_LEN];

        let level = append_tileset(
            &mut bytes,
            &SyntheticTilesetSpec {
                tile_count: 2,
                rotations: 3,
                cga_size: 11,
                ega_size: 12,
                vga_size: 13,
                bit_depth: 2,
                flags: FLAG_LEVEL_TILESET,
                color_map: Some(&[
                    [1, 10, 20, 0],
                    [2, 11, 21, 0],
                    [3, 12, 22, 0],
                    [4, 13, 23, 0],
                ]),
                tiles: &[
                    SyntheticTileSpec {
                        width: 2,
                        height: 2,
                        data_format: 7,
                        indexed_pixels: &[0, 1, 2, 3],
                    },
                    SyntheticTileSpec {
                        width: 1,
                        height: 3,
                        data_format: 9,
                        indexed_pixels: &[3, 2, 1],
                    },
                ],
            },
        );
        let font = append_tileset(
            &mut bytes,
            &SyntheticTilesetSpec {
                tile_count: 1,
                rotations: 1,
                cga_size: 21,
                ega_size: 22,
                vga_size: 23,
                bit_depth: 1,
                flags: FLAG_FONT,
                color_map: None,
                tiles: &[SyntheticTileSpec {
                    width: 1,
                    height: 2,
                    data_format: 0,
                    indexed_pixels: &[9, 8],
                }],
            },
        );

        write_header_entry(&mut bytes, 1, level.offset, level.size);
        write_header_entry(&mut bytes, 2, font.offset, font.size);
        write_header_entry(&mut bytes, 3, level.offset, 0);
        write_header_entry(&mut bytes, 4, 0, 9);

        SyntheticShaFixture {
            bytes,
            level_tileset_offset: level.offset,
            level_tileset_size: level.size,
            level_tile_offsets: [level.tile_offsets[0], level.tile_offsets[1]],
            font_tileset_offset: font.offset,
            font_tileset_size: font.size,
            font_tile_offset: font.tile_offsets[0],
        }
    }

    /// Builds a minimal SHA fixture whose only tile truncates in the indexed
    /// pixel payload, for error-offset assertions.
    fn truncated_tile_pixels_fixture() -> Vec<u8> {
        let mut bytes = vec![0; HEADER_LEN];
        let offset = u32::try_from(bytes.len()).expect("synthetic offsets must fit in u32");

        bytes.push(1);
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.extend(1u16.to_le_bytes());
        bytes.push(8);
        bytes.extend(FLAG_LEVEL_TILESET.to_le_bytes());
        bytes.push(2);
        bytes.push(2);
        bytes.push(0);
        bytes.extend([1, 2, 3]);

        let size = u16::try_from(bytes.len() - HEADER_LEN).expect("synthetic size must fit in u16");
        write_header_entry(&mut bytes, 1, offset, size);
        bytes
    }

    /// Writes one `(offset, size)` pair into the synthetic SHA header.
    fn write_header_entry(bytes: &mut [u8], index: usize, offset: u32, size: u16) {
        let offset_pos = index * 4;
        bytes[offset_pos..offset_pos + 4].copy_from_slice(&offset.to_le_bytes());

        let size_pos = (TILESET_ENTRY_COUNT * 4) + (index * 2);
        bytes[size_pos..size_pos + 2].copy_from_slice(&size.to_le_bytes());
    }

    /// Placement information returned by [`append_tileset`] so tests can assert
    /// offsets without re-deriving them from raw byte math.
    struct SyntheticTilesetPlacement {
        /// Byte offset at which the tileset starts.
        offset: u32,
        /// Byte size occupied by the tileset.
        size: u16,
        /// Byte offsets at which each tile's indexed pixels begin.
        tile_offsets: Vec<usize>,
    }

    /// Synthetic tile specification consumed by [`append_tileset`] when
    /// building unit-test fixtures.
    struct SyntheticTileSpec<'a> {
        /// Width encoded into the tile header.
        width: u8,
        /// Height encoded into the tile header.
        height: u8,
        /// Data-format byte encoded into the tile header.
        data_format: u8,
        /// Row-major indexed pixels written after the tile header.
        indexed_pixels: &'a [u8],
    }

    /// Synthetic tileset specification consumed by [`append_tileset`] when
    /// building unit-test fixtures.
    struct SyntheticTilesetSpec<'a> {
        /// Number of tiles declared in the tileset header.
        tile_count: u8,
        /// Rotations field preserved in the tileset header.
        rotations: u16,
        /// CGA decompressed-size field written into the tileset header.
        cga_size: u16,
        /// EGA decompressed-size field written into the tileset header.
        ega_size: u16,
        /// VGA decompressed-size field written into the tileset header.
        vga_size: u16,
        /// Bit depth written into the tileset header.
        bit_depth: u8,
        /// Flags written into the tileset header.
        flags: u16,
        /// Optional synthetic color map written after the tileset header.
        color_map: Option<&'a [[u8; COLOR_MAP_ENTRY_LEN]]>,
        /// Synthetic tile/image records written after the optional color map.
        tiles: &'a [SyntheticTileSpec<'a>],
    }

    /// Appends one synthetic tileset to `bytes`, returning the tileset's offset,
    /// size, and per-tile pixel-data offsets for later assertions.
    fn append_tileset(
        bytes: &mut Vec<u8>,
        spec: &SyntheticTilesetSpec<'_>,
    ) -> SyntheticTilesetPlacement {
        let offset = u32::try_from(bytes.len()).expect("synthetic offsets must fit in u32");
        bytes.push(spec.tile_count);
        bytes.extend(spec.rotations.to_le_bytes());
        bytes.extend(spec.cga_size.to_le_bytes());
        bytes.extend(spec.ega_size.to_le_bytes());
        bytes.extend(spec.vga_size.to_le_bytes());
        bytes.push(spec.bit_depth);
        bytes.extend(spec.flags.to_le_bytes());

        if let Some(color_map) = spec.color_map {
            for entry in color_map {
                bytes.extend(entry);
            }
        }

        let mut tile_offsets = Vec::with_capacity(spec.tiles.len());
        for tile in spec.tiles {
            bytes.push(tile.width);
            bytes.push(tile.height);
            bytes.push(tile.data_format);
            tile_offsets.push(bytes.len());
            bytes.extend(tile.indexed_pixels);
        }

        let size = u16::try_from(bytes.len() - usize::try_from(offset).expect("offset fits usize"))
            .expect("synthetic tileset size must fit in u16");
        SyntheticTilesetPlacement {
            offset,
            size,
            tile_offsets,
        }
    }
}
