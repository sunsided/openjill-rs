//! Parser for `*.JN1` level, map, save, object, and string-stack data.

use crate::{ByteReader, ByteReaderError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Width of the background map in cells.
pub const BACKGROUND_WIDTH: usize = 128;
/// Height of the background map in cells.
pub const BACKGROUND_HEIGHT: usize = 64;
/// Number of background cells stored in every JN file.
pub const BACKGROUND_CELL_COUNT: usize = BACKGROUND_WIDTH * BACKGROUND_HEIGHT;
/// Mask applied to each raw background map code.
pub const BACKGROUND_MAP_CODE_MASK: u16 = 0x0fff;
/// Fixed number of inventory slots in the save-data block.
pub const SAVE_INVENTORY_CAPACITY: usize = 16;
/// Number of unused bytes at the end of the save-data block.
pub const SAVE_HOLE_LEN: usize = 28;
/// Fixed byte length of the save-data block.
pub const SAVE_DATA_LEN: usize = 70;

/// One parsed 128x64 background layer from a JN file.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JnBackgroundLayer {
    /// Background map codes stored in OpenJill row order.
    map_codes: Vec<u16>,
}

impl JnBackgroundLayer {
    /// Returns the fixed background width in cells.
    pub fn width(&self) -> usize {
        BACKGROUND_WIDTH
    }

    /// Returns the fixed background height in cells.
    pub fn height(&self) -> usize {
        BACKGROUND_HEIGHT
    }

    /// Returns all masked background map codes in source order.
    pub fn map_codes(&self) -> &[u16] {
        &self.map_codes
    }

    /// Returns the masked map code at `x, y`, or `None` when out of bounds.
    pub fn map_code(&self, x: usize, y: usize) -> Option<u16> {
        if x < BACKGROUND_WIDTH && y < BACKGROUND_HEIGHT {
            Some(self.map_codes[background_index(x, y)])
        } else {
            None
        }
    }
}

/// One object record parsed from the object layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JnObject {
    /// Object type identifier.
    object_type: u8,
    /// X-coordinate stored in the object record.
    x: u16,
    /// Y-coordinate stored in the object record.
    y: u16,
    /// Signed horizontal speed or direction.
    x_speed: i16,
    /// Signed vertical speed or direction.
    y_speed: i16,
    /// Object width.
    width: u16,
    /// Object height.
    height: u16,
    /// Signed object state.
    state: i16,
    /// Object sub-state.
    sub_state: u16,
    /// Object state counter.
    state_count: u16,
    /// Signed object counter.
    counter: i16,
    /// Object flag bits.
    flags: u16,
    /// Raw string-stack pointer marker.
    pointer: u32,
    /// Signed auxiliary object field.
    info1: i16,
    /// Collision/hold auxiliary object field.
    zap_hold: u16,
    /// Position of this object in the object layer.
    index: usize,
    /// Source byte offset where this object record starts.
    offset: usize,
    /// Index of the associated string-stack entry, when the pointer marker is nonzero.
    string_index: Option<usize>,
}

impl JnObject {
    /// Returns the object type identifier.
    pub fn object_type(&self) -> u8 {
        self.object_type
    }

    /// Returns the X-coordinate.
    pub fn x(&self) -> u16 {
        self.x
    }

    /// Returns the Y-coordinate.
    pub fn y(&self) -> u16 {
        self.y
    }

    /// Returns the signed horizontal speed or direction.
    pub fn x_speed(&self) -> i16 {
        self.x_speed
    }

    /// Returns the signed vertical speed or direction.
    pub fn y_speed(&self) -> i16 {
        self.y_speed
    }

    /// Returns the object width.
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Returns the object height.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// Returns the signed object state.
    pub fn state(&self) -> i16 {
        self.state
    }

    /// Returns the object sub-state.
    pub fn sub_state(&self) -> u16 {
        self.sub_state
    }

    /// Returns the object state counter.
    pub fn state_count(&self) -> u16 {
        self.state_count
    }

    /// Returns the signed object counter.
    pub fn counter(&self) -> i16 {
        self.counter
    }

    /// Returns the object flag bits.
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Returns the raw string-stack pointer marker.
    pub fn pointer(&self) -> u32 {
        self.pointer
    }

    /// Returns the signed auxiliary object field.
    pub fn info1(&self) -> i16 {
        self.info1
    }

    /// Returns the collision/hold auxiliary object field.
    pub fn zap_hold(&self) -> u16 {
        self.zap_hold
    }

    /// Returns the position of this object in the object layer.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the source byte offset where this object record starts.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the associated string-stack entry index, if one was assigned.
    pub fn string_index(&self) -> Option<usize> {
        self.string_index
    }
}

/// Fixed save-data block parsed after the object layer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JnSaveData {
    /// Displayed level number.
    level: u16,
    /// Player health value.
    health: u16,
    /// Inventory items present in the save data.
    inventory: Vec<u16>,
    /// Unused inventory padding slots up to the fixed capacity.
    inventory_padding: Vec<u16>,
    /// Player score value.
    score: u32,
    /// Remaining unused save-data bytes.
    hole: Vec<u8>,
    /// Source byte offset where this save-data block starts.
    offset: usize,
}

impl JnSaveData {
    /// Returns the displayed level number.
    pub fn level(&self) -> u16 {
        self.level
    }

    /// Returns the player health value.
    pub fn health(&self) -> u16 {
        self.health
    }

    /// Returns inventory items in source order.
    pub fn inventory(&self) -> &[u16] {
        &self.inventory
    }

    /// Returns fixed-capacity inventory padding values.
    pub fn inventory_padding(&self) -> &[u16] {
        &self.inventory_padding
    }

    /// Returns the player score value.
    pub fn score(&self) -> u32 {
        self.score
    }

    /// Returns the remaining unused save-data bytes.
    pub fn hole(&self) -> &[u8] {
        &self.hole
    }

    /// Returns the source byte offset where this save-data block starts.
    pub fn offset(&self) -> usize {
        self.offset
    }
}

/// One length-prefixed string-stack entry parsed after save data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JnString {
    /// String value decoded byte-for-byte as `U+0000..U+00FF` characters.
    value: String,
    /// Raw trailing terminator byte.
    terminator: u8,
    /// Source byte offset where this string record starts.
    offset: usize,
}

impl JnString {
    /// Returns the decoded string value.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// Returns the raw trailing terminator byte.
    pub fn terminator(&self) -> u8 {
        self.terminator
    }

    /// Returns the source byte offset where this string record starts.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Returns the number of bytes this string occupied in the file.
    pub fn size_in_file(&self) -> usize {
        2 + self.value.chars().count() + 1
    }
}

/// Parsed JN file data: background, objects, save data, and string stack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JnFile {
    /// Parsed background layer.
    background: JnBackgroundLayer,
    /// Parsed object records in source order.
    objects: Vec<JnObject>,
    /// Parsed fixed save-data block.
    save_data: JnSaveData,
    /// Parsed string-stack entries in source order.
    strings: Vec<JnString>,
}

impl JnFile {
    /// Parses a JN file from the supplied reader.
    pub fn parse(reader: &mut ByteReader) -> Result<Self, JnReadError> {
        let background = parse_background(reader)?;
        let object_count = read_u16(reader, "object_count", None, None)? as usize;
        let mut objects = Vec::with_capacity(object_count);

        for object_index in 0..object_count {
            objects.push(parse_object(reader, object_index)?);
        }

        let save_data = parse_save_data(reader)?;
        let strings = parse_strings(reader, &mut objects)?;

        Ok(Self {
            background,
            objects,
            save_data,
            strings,
        })
    }

    /// Parses a `JnFile` directly from in-memory bytes.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, JnReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader)
    }

    /// Returns the parsed background layer.
    pub fn background(&self) -> &JnBackgroundLayer {
        &self.background
    }

    /// Returns parsed object records in source order.
    pub fn objects(&self) -> &[JnObject] {
        &self.objects
    }

    /// Returns parsed save data.
    pub fn save_data(&self) -> &JnSaveData {
        &self.save_data
    }

    /// Returns parsed string-stack entries in source order.
    pub fn strings(&self) -> &[JnString] {
        &self.strings
    }
}

/// Error returned when parsing a JN file fails.
#[derive(Debug, Eq, PartialEq)]
pub struct JnReadError {
    /// Name of the field being parsed when the failure occurred.
    pub field: &'static str,
    /// Optional object index for object-scoped failures.
    pub object_index: Option<usize>,
    /// Optional string index for string-scoped failures.
    pub string_index: Option<usize>,
    /// Source offset associated with the parse failure.
    pub offset: usize,
    /// Underlying byte-reader failure.
    source: ByteReaderError,
}

impl Display for JnReadError {
    /// Formats the error including field, optional indices, offset, and cause.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.object_index, self.string_index) {
            (Some(object_index), _) => write!(
                f,
                "failed to parse JN field '{}' for object {} at offset {}: {}",
                self.field, object_index, self.offset, self.source
            ),
            (_, Some(string_index)) => write!(
                f,
                "failed to parse JN field '{}' for string {} at offset {}: {}",
                self.field, string_index, self.offset, self.source
            ),
            (None, None) => write!(
                f,
                "failed to parse JN field '{}' at offset {}: {}",
                self.field, self.offset, self.source
            ),
        }
    }
}

impl Error for JnReadError {
    /// Returns the underlying byte-reader error as the cause of this failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Parses the fixed-size masked background layer.
fn parse_background(reader: &mut ByteReader) -> Result<JnBackgroundLayer, JnReadError> {
    let mut map_codes = Vec::with_capacity(BACKGROUND_CELL_COUNT);

    for _ in 0..BACKGROUND_CELL_COUNT {
        let raw = read_u16(reader, "background_map_code", None, None)?;
        map_codes.push(raw & BACKGROUND_MAP_CODE_MASK);
    }

    Ok(JnBackgroundLayer { map_codes })
}

/// Parses one object record in OpenJill field order.
fn parse_object(reader: &mut ByteReader, object_index: usize) -> Result<JnObject, JnReadError> {
    let offset = reader.offset();

    Ok(JnObject {
        object_type: read_u8(reader, "object_type", Some(object_index), None)?,
        x: read_u16(reader, "x", Some(object_index), None)?,
        y: read_u16(reader, "y", Some(object_index), None)?,
        x_speed: read_i16(reader, "x_speed", Some(object_index), None)?,
        y_speed: read_i16(reader, "y_speed", Some(object_index), None)?,
        width: read_u16(reader, "width", Some(object_index), None)?,
        height: read_u16(reader, "height", Some(object_index), None)?,
        state: read_i16(reader, "state", Some(object_index), None)?,
        sub_state: read_u16(reader, "sub_state", Some(object_index), None)?,
        state_count: read_u16(reader, "state_count", Some(object_index), None)?,
        counter: read_i16(reader, "counter", Some(object_index), None)?,
        flags: read_u16(reader, "flags", Some(object_index), None)?,
        pointer: read_u32(reader, "pointer", Some(object_index), None)?,
        info1: read_i16(reader, "info1", Some(object_index), None)?,
        zap_hold: read_u16(reader, "zap_hold", Some(object_index), None)?,
        index: object_index,
        offset,
        string_index: None,
    })
}

/// Parses the fixed 70-byte save-data block.
fn parse_save_data(reader: &mut ByteReader) -> Result<JnSaveData, JnReadError> {
    let offset = reader.offset();
    let level = read_u16(reader, "save_level", None, None)?;
    let health = read_u16(reader, "save_health", None, None)?;
    let inventory_count = read_u16(reader, "save_inventory_count", None, None)? as usize;
    let item_count = inventory_count.min(SAVE_INVENTORY_CAPACITY);
    let mut inventory = Vec::with_capacity(item_count);

    for _ in 0..item_count {
        inventory.push(read_u16(reader, "save_inventory_item", None, None)?);
    }

    let padding_count = SAVE_INVENTORY_CAPACITY.saturating_sub(item_count);
    let mut inventory_padding = Vec::with_capacity(padding_count);
    for _ in 0..padding_count {
        inventory_padding.push(read_u16(reader, "save_inventory_padding", None, None)?);
    }

    let score = read_u32(reader, "save_score", None, None)?;
    let mut hole = Vec::with_capacity(SAVE_HOLE_LEN);
    for _ in 0..SAVE_HOLE_LEN {
        hole.push(read_u8(reader, "save_hole", None, None)?);
    }

    Ok(JnSaveData {
        level,
        health,
        inventory,
        inventory_padding,
        score,
        hole,
        offset,
    })
}

/// Parses string-stack entries to EOF and associates them with pointed objects.
fn parse_strings(
    reader: &mut ByteReader,
    objects: &mut [JnObject],
) -> Result<Vec<JnString>, JnReadError> {
    let mut strings = Vec::new();
    let mut next_object_index = 0;

    while reader.offset() < reader.len() {
        let string_index = strings.len();
        let string = parse_string(reader, string_index)?;

        if let Some(object_index) = next_object_with_string(objects, next_object_index) {
            objects[object_index].string_index = Some(string_index);
            next_object_index = object_index + 1;
        }

        strings.push(string);
    }

    Ok(strings)
}

/// Parses one length-prefixed, null-terminated string-stack entry.
fn parse_string(reader: &mut ByteReader, string_index: usize) -> Result<JnString, JnReadError> {
    let offset = reader.offset();
    let len = read_u16(reader, "string_len", None, Some(string_index))? as usize;
    let mut value = String::with_capacity(len);

    for _ in 0..len {
        value.push(char::from(read_u8(
            reader,
            "string_byte",
            None,
            Some(string_index),
        )?));
    }

    let terminator = read_u8(reader, "string_terminator", None, Some(string_index))?;

    Ok(JnString {
        value,
        terminator,
        offset,
    })
}

/// Finds the next object whose raw pointer field is nonzero.
fn next_object_with_string(objects: &[JnObject], start_index: usize) -> Option<usize> {
    objects
        .iter()
        .enumerate()
        .skip(start_index)
        .find_map(|(index, object)| (object.pointer != 0).then_some(index))
}

/// Computes the row-order background index for `x, y`.
fn background_index(x: usize, y: usize) -> usize {
    (x * BACKGROUND_HEIGHT) + y
}

/// Reads a single `u8` field and wraps reader errors with JN parse context.
fn read_u8(
    reader: &mut ByteReader,
    field: &'static str,
    object_index: Option<usize>,
    string_index: Option<usize>,
) -> Result<u8, JnReadError> {
    let offset = reader.offset();
    reader.read_u8().map_err(|source| JnReadError {
        field,
        object_index,
        string_index,
        offset: error_offset(&source, offset),
        source,
    })
}

/// Reads a single `u16le` field and wraps reader errors with JN parse context.
fn read_u16(
    reader: &mut ByteReader,
    field: &'static str,
    object_index: Option<usize>,
    string_index: Option<usize>,
) -> Result<u16, JnReadError> {
    let offset = reader.offset();
    reader.read_u16_le().map_err(|source| JnReadError {
        field,
        object_index,
        string_index,
        offset: error_offset(&source, offset),
        source,
    })
}

/// Reads a single `i16le` field and wraps reader errors with JN parse context.
fn read_i16(
    reader: &mut ByteReader,
    field: &'static str,
    object_index: Option<usize>,
    string_index: Option<usize>,
) -> Result<i16, JnReadError> {
    let offset = reader.offset();
    reader.read_i16_le().map_err(|source| JnReadError {
        field,
        object_index,
        string_index,
        offset: error_offset(&source, offset),
        source,
    })
}

/// Reads a single `u32le` field and wraps reader errors with JN parse context.
fn read_u32(
    reader: &mut ByteReader,
    field: &'static str,
    object_index: Option<usize>,
    string_index: Option<usize>,
) -> Result<u32, JnReadError> {
    let offset = reader.offset();
    reader.read_u32_le().map_err(|source| JnReadError {
        field,
        object_index,
        string_index,
        offset: error_offset(&source, offset),
        source,
    })
}

/// Chooses the most useful parse-failure offset to report for a reader error.
fn error_offset(source: &ByteReaderError, lower_bound_offset: usize) -> usize {
    match source {
        ByteReaderError::UnexpectedEof { offset, .. }
        | ByteReaderError::InvalidSeek {
            requested: offset, ..
        }
        | ByteReaderError::OffsetOverflow { offset, .. } => *offset,
    }
    .max(lower_bound_offset)
}

#[cfg(test)]
mod tests {
    use super::{
        BACKGROUND_CELL_COUNT, BACKGROUND_HEIGHT, BACKGROUND_MAP_CODE_MASK, JnFile, SAVE_HOLE_LEN,
        SAVE_INVENTORY_CAPACITY,
    };
    use assert2::check;

    /// Unit under test: `JnFile::parse` background layer extraction.
    ///
    /// Preconditions: a synthetic JN buffer contains a full background layer
    /// with high flag bits set on selected cells, zero objects, a valid empty
    /// save-data block, and no string stack.
    ///
    /// Invariants asserted: dimensions are fixed at 128x64, source order is
    /// preserved, and every returned map code is masked to the lower 12 bits.
    #[test]
    fn parses_background_dimensions_and_masks_map_codes() {
        let mut bytes = Vec::new();
        write_background(&mut bytes, |cell| {
            if cell == 0 {
                0xf234
            } else if cell == BACKGROUND_HEIGHT {
                0x8abc
            } else {
                cell as u16
            }
        });
        write_u16(&mut bytes, 0);
        write_save_data(&mut bytes, 1, 100, &[], 0x0102_0304);

        let jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        check!(jn.background().width() == 128);
        check!(jn.background().height() == 64);
        check!(jn.background().map_codes().len() == BACKGROUND_CELL_COUNT);
        check!(jn.background().map_code(0, 0) == Some(0x0234));
        check!(jn.background().map_code(1, 0) == Some(0x0abc));
        check!(jn.background().map_code(128, 0).is_none());
        check!(
            jn.background()
                .map_codes()
                .iter()
                .all(|map_code| map_code & !BACKGROUND_MAP_CODE_MASK == 0)
        );
    }

    /// Unit under test: `JnFile::parse` object layer extraction.
    ///
    /// Preconditions: a synthetic JN buffer contains two object records with
    /// different pointer markers and signed fields, followed by a valid empty
    /// save-data block.
    ///
    /// Invariants asserted: object count, field order, preserved source
    /// indexes, record offsets, and signed values match the fixture exactly.
    #[test]
    fn parses_object_count_fields_indexes_offsets_and_signed_values() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 2);
        let first_offset = bytes.len();
        write_object(&mut bytes, ObjectFixture::sample(3, -7, -11, 0));
        let second_offset = bytes.len();
        write_object(&mut bytes, ObjectFixture::sample(4, -300, -301, 99));
        write_save_data(&mut bytes, 2, 99, &[], 1234);

        let jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        check!(jn.objects().len() == 2);
        check!(jn.objects()[0].index() == 0);
        check!(jn.objects()[0].offset() == first_offset);
        check!(jn.objects()[0].object_type() == 3);
        check!(jn.objects()[0].x_speed() == -7);
        check!(jn.objects()[0].y_speed() == -11);
        check!(jn.objects()[0].pointer() == 0);
        check!(jn.objects()[1].index() == 1);
        check!(jn.objects()[1].offset() == second_offset);
        check!(jn.objects()[1].object_type() == 4);
        check!(jn.objects()[1].counter() == -300);
        check!(jn.objects()[1].info1() == -301);
        check!(jn.objects()[1].pointer() == 99);
    }

    /// Unit under test: `JnFile::parse` save-data extraction.
    ///
    /// Preconditions: a synthetic JN buffer contains zero objects and a
    /// save-data block with three inventory entries, fixed-capacity padding,
    /// a score, and the 28-byte trailing hole.
    ///
    /// Invariants asserted: level, health, inventory entries, padding length,
    /// score, hole length, and save-data offset match the fixed layout.
    #[test]
    fn parses_save_data_inventory_padding_score_and_hole() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 0);
        let save_offset = bytes.len();
        write_save_data(&mut bytes, 7, 42, &[10, 11, 12], 987_654);

        let jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        check!(jn.save_data().offset() == save_offset);
        check!(jn.save_data().level() == 7);
        check!(jn.save_data().health() == 42);
        check!(jn.save_data().inventory() == [10, 11, 12]);
        check!(jn.save_data().inventory_padding().len() == SAVE_INVENTORY_CAPACITY - 3);
        check!(jn.save_data().score() == 987_654);
        check!(jn.save_data().hole().len() == SAVE_HOLE_LEN);
        check!(jn.save_data().hole().iter().all(|byte| *byte == 0xa5));
    }

    /// Unit under test: `JnFile::parse` string-stack extraction and object
    /// association.
    ///
    /// Preconditions: a synthetic JN buffer contains three objects, two with
    /// nonzero pointer markers, followed by two string-stack entries.
    ///
    /// Invariants asserted: strings are decoded to EOF, offsets and sizes are
    /// preserved, and string entries attach to pointed objects in object
    /// iteration order while unpointed objects remain unassociated.
    #[test]
    fn parses_strings_and_associates_them_with_pointed_objects_in_order() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 3);
        write_object(&mut bytes, ObjectFixture::sample(1, 0, 0, 17));
        write_object(&mut bytes, ObjectFixture::sample(2, 0, 0, 0));
        write_object(&mut bytes, ObjectFixture::sample(3, 0, 0, 99));
        write_save_data(&mut bytes, 4, 77, &[], 44);
        let first_string_offset = bytes.len();
        write_string(&mut bytes, b"HELLO");
        let second_string_offset = bytes.len();
        write_string(&mut bytes, &[b'B', 0, 0xff]);

        let jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        check!(jn.strings().len() == 2);
        check!(jn.strings()[0].offset() == first_string_offset);
        check!(jn.strings()[0].value() == "HELLO");
        check!(jn.strings()[0].terminator() == 0);
        check!(jn.strings()[0].size_in_file() == 8);
        check!(jn.strings()[1].offset() == second_string_offset);
        check!(jn.strings()[1].value().as_bytes() == [b'B', 0, 0xc3, 0xbf]);
        check!(jn.objects()[0].string_index() == Some(0));
        check!(jn.objects()[1].string_index().is_none());
        check!(jn.objects()[2].string_index() == Some(1));
    }

    /// Unit under test: `JnReadError` offset reporting for truncated object data.
    ///
    /// Preconditions: a synthetic JN buffer declares one object but only
    /// provides the first byte of its record.
    ///
    /// Invariants asserted: parsing fails on the `x` field for object zero
    /// and reports the byte offset where that failing field started.
    #[test]
    fn includes_failing_offset_when_object_record_is_truncated() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 1);
        let failing_offset = bytes.len() + 1;
        bytes.push(9);

        let Err(err) = JnFile::from_bytes(bytes) else {
            panic!("truncated object record should fail");
        };

        check!(err.field == "x");
        check!(err.object_index == Some(0));
        check!(err.offset == failing_offset);
    }

    /// Builds the fixed zero-filled background fixture.
    fn base_background() -> Vec<u8> {
        let mut bytes = Vec::new();
        write_background(&mut bytes, |_| 0);
        bytes
    }

    /// Writes a full background layer using `value_for_cell`.
    fn write_background(bytes: &mut Vec<u8>, value_for_cell: impl Fn(usize) -> u16) {
        for cell in 0..BACKGROUND_CELL_COUNT {
            write_u16(bytes, value_for_cell(cell));
        }
    }

    /// Writes one object record in OpenJill field order.
    fn write_object(bytes: &mut Vec<u8>, fixture: ObjectFixture) {
        bytes.push(fixture.object_type);
        write_u16(bytes, fixture.x);
        write_u16(bytes, fixture.y);
        write_i16(bytes, fixture.x_speed);
        write_i16(bytes, fixture.y_speed);
        write_u16(bytes, fixture.width);
        write_u16(bytes, fixture.height);
        write_i16(bytes, fixture.state);
        write_u16(bytes, fixture.sub_state);
        write_u16(bytes, fixture.state_count);
        write_i16(bytes, fixture.counter);
        write_u16(bytes, fixture.flags);
        bytes.extend_from_slice(&fixture.pointer.to_le_bytes());
        write_i16(bytes, fixture.info1);
        write_u16(bytes, fixture.zap_hold);
    }

    /// Writes one fixed-size save-data block.
    fn write_save_data(
        bytes: &mut Vec<u8>,
        level: u16,
        health: u16,
        inventory: &[u16],
        score: u32,
    ) {
        write_u16(bytes, level);
        write_u16(bytes, health);
        write_u16(bytes, inventory.len() as u16);
        for item in inventory {
            write_u16(bytes, *item);
        }
        for index in inventory.len()..SAVE_INVENTORY_CAPACITY {
            write_u16(bytes, 0xf000 | index as u16);
        }
        bytes.extend_from_slice(&score.to_le_bytes());
        bytes.extend(std::iter::repeat_n(0xa5, SAVE_HOLE_LEN));
    }

    /// Writes one length-prefixed, null-terminated string-stack entry.
    fn write_string(bytes: &mut Vec<u8>, value: &[u8]) {
        write_u16(bytes, value.len() as u16);
        bytes.extend_from_slice(value);
        bytes.push(0);
    }

    /// Writes one `u16le` value.
    fn write_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Writes one `i16le` value.
    fn write_i16(bytes: &mut Vec<u8>, value: i16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    /// Synthetic object field bundle used by fixture writers.
    struct ObjectFixture {
        /// Object type identifier.
        object_type: u8,
        /// X-coordinate.
        x: u16,
        /// Y-coordinate.
        y: u16,
        /// Signed horizontal speed or direction.
        x_speed: i16,
        /// Signed vertical speed or direction.
        y_speed: i16,
        /// Object width.
        width: u16,
        /// Object height.
        height: u16,
        /// Signed object state.
        state: i16,
        /// Object sub-state.
        sub_state: u16,
        /// Object state counter.
        state_count: u16,
        /// Signed object counter.
        counter: i16,
        /// Object flags.
        flags: u16,
        /// Raw string-stack pointer marker.
        pointer: u32,
        /// Signed auxiliary field.
        info1: i16,
        /// Collision/hold auxiliary field.
        zap_hold: u16,
    }

    impl ObjectFixture {
        /// Creates a sample object fixture with selected signed and pointer fields.
        fn sample(object_type: u8, counter: i16, info1: i16, pointer: u32) -> Self {
            Self {
                object_type,
                x: 10,
                y: 20,
                x_speed: counter,
                y_speed: info1,
                width: 30,
                height: 40,
                state: -5,
                sub_state: 6,
                state_count: 7,
                counter,
                flags: 8,
                pointer,
                info1,
                zap_hold: 9,
            }
        }
    }
}
