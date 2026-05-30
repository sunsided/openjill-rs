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

    /// Creates an object record for a runtime-spawned object that has no parsed
    /// source position (player-thrown projectiles, death-scatter particles).
    ///
    /// The parse-bookkeeping fields (`index`, `offset`, `string_index`) are
    /// synthetic and `pointer` is `0` (no string-stack entry); all live state
    /// fields start at `0` and are set by the spawning entity before a save.
    pub fn spawned(object_type: u8, x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            object_type,
            x,
            y,
            x_speed: 0,
            y_speed: 0,
            width,
            height,
            state: 0,
            sub_state: 0,
            state_count: 0,
            counter: 0,
            flags: 0,
            pointer: 0,
            info1: 0,
            zap_hold: 0,
            index: 0,
            offset: 0,
            string_index: None,
        }
    }

    /// Sets the object position in pixels.
    pub fn set_position(&mut self, x: u16, y: u16) {
        self.x = x;
        self.y = y;
    }

    /// Sets the object dimensions in pixels.
    pub fn set_dimensions(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    /// Sets the signed horizontal and vertical speed fields.
    pub fn set_speed(&mut self, x_speed: i16, y_speed: i16) {
        self.x_speed = x_speed;
        self.y_speed = y_speed;
    }

    /// Sets the signed object state field.
    pub fn set_state(&mut self, state: i16) {
        self.state = state;
    }

    /// Sets the object sub-state field.
    pub fn set_sub_state(&mut self, sub_state: u16) {
        self.sub_state = sub_state;
    }

    /// Sets the object state-counter field.
    pub fn set_state_count(&mut self, state_count: u16) {
        self.state_count = state_count;
    }

    /// Sets the signed object counter field.
    pub fn set_counter(&mut self, counter: i16) {
        self.counter = counter;
    }

    /// Sets the object flag bits.
    pub fn set_flags(&mut self, flags: u16) {
        self.flags = flags;
    }

    /// Sets the signed auxiliary `info1` field.
    pub fn set_info1(&mut self, info1: i16) {
        self.info1 = info1;
    }

    /// Sets the collision/hold auxiliary `zap_hold` field.
    pub fn set_zap_hold(&mut self, zap_hold: u16) {
        self.zap_hold = zap_hold;
    }

    /// Appends this object record to `out` in OpenJill field order (matching
    /// `parse_object` and the Java `writeToFile`).
    fn write_to(&self, out: &mut Vec<u8>) {
        out.push(self.object_type);
        put_u16(out, self.x);
        put_u16(out, self.y);
        put_i16(out, self.x_speed);
        put_i16(out, self.y_speed);
        put_u16(out, self.width);
        put_u16(out, self.height);
        put_i16(out, self.state);
        put_u16(out, self.sub_state);
        put_u16(out, self.state_count);
        put_i16(out, self.counter);
        put_u16(out, self.flags);
        out.extend_from_slice(&self.pointer.to_le_bytes());
        put_i16(out, self.info1);
        put_u16(out, self.zap_hold);
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

    /// Appends the fixed 70-byte save-data block to `out`, mirroring the Java
    /// `writeSaveDataInFile`: level, health, inventory count, inventory items,
    /// fixed-capacity padding, score, then the trailing hole.
    fn write_to(&self, out: &mut Vec<u8>) {
        put_u16(out, self.level);
        put_u16(out, self.health);
        put_u16(out, self.inventory.len() as u16);
        for &item in &self.inventory {
            put_u16(out, item);
        }
        for &pad in &self.inventory_padding {
            put_u16(out, pad);
        }
        out.extend_from_slice(&self.score.to_le_bytes());
        out.extend_from_slice(&self.hole);
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

    /// Appends this length-prefixed, terminated string-stack entry to `out`.
    ///
    /// Each character was decoded from a single source byte (`U+0000..U+00FF`)
    /// so writing `ch as u8` is lossless.
    fn write_to(&self, out: &mut Vec<u8>) {
        put_u16(out, self.value.chars().count() as u16);
        for ch in self.value.chars() {
            out.push(ch as u8);
        }
        out.push(self.terminator);
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

    /// Serializes this file back to JN bytes in the original field order,
    /// mirroring the Java `AbstractChangeLevel` save path
    /// (`writeBackgroundInFile` -> `writeObjectInFile` -> `writeSaveDataInFile`
    /// followed by the string stack).
    ///
    /// Background codes are emitted masked to [`BACKGROUND_MAP_CODE_MASK`], the
    /// same canonical form DOS Jill writes on save (`getMapCode` returns the
    /// masked value), so the output matches the game's save-file layout.  A
    /// distribution level file that carries high background bits therefore
    /// re-serializes to its masked form, not byte-for-byte to the original;
    /// `parse -> to_bytes -> parse` is the stable round-trip.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        // Background layer (stored column-major: x outer, y inner).
        for &code in &self.background.map_codes {
            put_u16(&mut bytes, code & BACKGROUND_MAP_CODE_MASK);
        }
        // Object layer: count then each record in source order.
        put_u16(&mut bytes, self.objects.len() as u16);
        for object in &self.objects {
            object.write_to(&mut bytes);
        }
        // Fixed save-data block.
        self.save_data.write_to(&mut bytes);
        // String stack to EOF.
        for string in &self.strings {
            string.write_to(&mut bytes);
        }
        bytes
    }

    /// Overwrites the background map code at `(x, y)` with `code` (masked to
    /// [`BACKGROUND_MAP_CODE_MASK`]); out-of-bounds coordinates are ignored.
    ///
    /// Used when building a live save snapshot from the running level's
    /// background grid (e.g. so opened doors persist).
    pub fn set_background_code(&mut self, x: usize, y: usize, code: u16) {
        if x < BACKGROUND_WIDTH && y < BACKGROUND_HEIGHT {
            self.background.map_codes[background_index(x, y)] = code & BACKGROUND_MAP_CODE_MASK;
        }
    }

    /// Replaces the object list, e.g. with the live entity snapshots that make
    /// up a save game.
    pub fn set_objects(&mut self, objects: Vec<JnObject>) {
        self.objects = objects;
    }

    /// Appends `object` to the object layer (the level editor's "add object"
    /// command) and returns its new index.
    ///
    /// String-stack entries link to objects positionally: the n-th object with
    /// a nonzero `pointer` owns the n-th string. An object with a zero `pointer`
    /// (e.g. from [`JnObject::spawned`], and every object the in-game editor
    /// places) carries no string, so appending it keeps that linkage intact
    /// across [`to_bytes`](Self::to_bytes) and a re-parse. Appending an object
    /// with a nonzero `pointer` additionally requires its string to be appended
    /// to the stack, since this method touches only the object layer.
    pub fn push_object(&mut self, object: JnObject) -> usize {
        let index = self.objects.len();
        self.objects.push(object);
        index
    }

    /// Removes and returns the object at `index` (the level editor's "delete
    /// object" command), or `None` when `index` is out of range.
    ///
    /// When the removed object owns a string-stack entry (nonzero `pointer`),
    /// that entry is pruned too, so the positional object-to-string linkage that
    /// [`to_bytes`](Self::to_bytes) relies on stays consistent across a
    /// re-parse. The owned string's position is recomputed from the surviving
    /// object order on each call (not from a cached index), so repeated removals
    /// remain correct.
    pub fn remove_object(&mut self, index: usize) -> Option<JnObject> {
        if index >= self.objects.len() {
            return None;
        }
        let removed = self.objects.remove(index);
        if removed.pointer != 0 {
            // Objects `[0, index)` are unchanged by the removal, so the count of
            // string-owning objects in that prefix is the removed object's rank
            // among string owners - the index of the string it owned.
            let string_rank = self.objects[..index]
                .iter()
                .filter(|object| object.pointer != 0)
                .count();
            if string_rank < self.strings.len() {
                self.strings.remove(string_rank);
            }
        }
        Some(removed)
    }

    /// Rewrites the save-data block from live runtime state.
    ///
    /// `inventory` is the list of `EnumInventoryObject` indices (capped at
    /// [`SAVE_INVENTORY_CAPACITY`]); the remaining inventory slots and the
    /// trailing hole are zero-filled, matching the Java reference's fresh-save
    /// `writeSaveDataInFile` output.  The source byte offset is preserved.
    pub fn set_save_data(&mut self, level: u16, health: u16, inventory: &[u16], score: u32) {
        let inventory: Vec<u16> = inventory
            .iter()
            .copied()
            .take(SAVE_INVENTORY_CAPACITY)
            .collect();
        let inventory_padding = vec![0u16; SAVE_INVENTORY_CAPACITY - inventory.len()];
        self.save_data = JnSaveData {
            level,
            health,
            inventory,
            inventory_padding,
            score,
            hole: vec![0u8; SAVE_HOLE_LEN],
            offset: self.save_data.offset,
        };
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

/// Appends a `u16` as little-endian bytes.
fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// Appends an `i16` as little-endian bytes.
fn put_i16(out: &mut Vec<u8>, value: i16) {
    out.extend_from_slice(&value.to_le_bytes());
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

    /// Unit under test: [`JnFile::to_bytes`] reproduces a synthetic source file
    /// byte-for-byte.
    ///
    /// Preconditions: a synthetic JN buffer with a clean (high-bits-zero)
    /// background, two object records (one carrying a string pointer), a
    /// save-data block with inventory entries, and two string-stack entries.
    ///
    /// Invariant asserted: `parse` then `to_bytes` returns the exact input
    /// bytes, so background, object layer, save-data block, and string stack
    /// all serialize in the original field order and widths.
    #[test]
    fn to_bytes_round_trips_a_synthetic_file_byte_for_byte() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 2);
        write_object(&mut bytes, ObjectFixture::sample(1, -7, -11, 17));
        write_object(&mut bytes, ObjectFixture::sample(2, 300, -301, 0));
        write_save_data(&mut bytes, 7, 42, &[10, 11, 12], 987_654);
        write_string(&mut bytes, b"HELLO");
        write_string(&mut bytes, &[b'B', 0, 0xff]);

        let jn = JnFile::from_bytes(bytes.clone()).expect("JN parse should succeed");

        check!(jn.to_bytes() == bytes);
    }

    /// Unit under test: [`JnFile::set_background_code`], [`JnFile::set_objects`],
    /// and [`JnFile::set_save_data`] (the live save-snapshot mutators).
    ///
    /// Invariant asserted: the mutations survive a `to_bytes -> parse`
    /// round-trip - the background code is masked, the object list is replaced,
    /// and the save-data block reflects the new runtime state.
    #[test]
    fn set_background_objects_and_save_data_round_trip_through_to_bytes() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 1);
        write_object(&mut bytes, ObjectFixture::sample(5, 1, 2, 0));
        write_save_data(&mut bytes, 1, 6, &[], 0);
        let mut jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        jn.set_background_code(2, 3, 0xFABC); // high nibble masked off -> 0x0ABC
        let mut obj = super::JnObject::spawned(9, 100, 50, 16, 16);
        obj.set_counter(7);
        jn.set_objects(vec![obj]);
        jn.set_save_data(4, 8, &[1, 3, 8], 12_345);

        let reparsed = JnFile::from_bytes(jn.to_bytes()).expect("re-parse should succeed");
        check!(reparsed.background().map_code(2, 3) == Some(0x0ABC));
        check!(reparsed.objects().len() == 1);
        check!(reparsed.objects()[0].object_type() == 9);
        check!(reparsed.objects()[0].counter() == 7);
        check!(reparsed.save_data().level() == 4);
        check!(reparsed.save_data().health() == 8);
        check!(reparsed.save_data().inventory() == [1, 3, 8]);
        check!(reparsed.save_data().score() == 12_345);
        check!(reparsed.save_data().hole().iter().all(|byte| *byte == 0));
    }

    /// Unit under test: [`JnFile::push_object`].
    ///
    /// Invariant asserted: a pushed (stringless) object is appended at the
    /// returned index and survives a `to_bytes -> parse` round-trip with its
    /// fields intact, leaving the rest of the layer untouched.
    #[test]
    fn push_object_appends_and_round_trips() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 1);
        write_object(&mut bytes, ObjectFixture::sample(5, 1, 2, 0));
        write_save_data(&mut bytes, 1, 6, &[], 0);
        let mut jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        let mut obj = super::JnObject::spawned(9, 100, 50, 16, 16);
        obj.set_counter(7);
        let new_index = jn.push_object(obj);
        check!(new_index == 1);

        let reparsed = JnFile::from_bytes(jn.to_bytes()).expect("re-parse should succeed");
        check!(reparsed.objects().len() == 2);
        check!(reparsed.objects()[1].object_type() == 9);
        check!(reparsed.objects()[1].x() == 100);
        check!(reparsed.objects()[1].counter() == 7);
    }

    /// Unit under test: [`JnFile::remove_object`] on a stringless object.
    ///
    /// Invariant asserted: the record is dropped, the returned object is the one
    /// that was removed, and the shortened layer round-trips through `to_bytes`.
    #[test]
    fn remove_object_drops_record_and_round_trips() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 2);
        write_object(&mut bytes, ObjectFixture::sample(5, 1, 2, 0));
        write_object(&mut bytes, ObjectFixture::sample(6, 3, 4, 0));
        write_save_data(&mut bytes, 1, 6, &[], 0);
        let mut jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        let removed = jn.remove_object(0).expect("object 0 should exist");
        check!(removed.object_type() == 5);

        let reparsed = JnFile::from_bytes(jn.to_bytes()).expect("re-parse should succeed");
        check!(reparsed.objects().len() == 1);
        check!(reparsed.objects()[0].object_type() == 6);
    }

    /// Unit under test: [`JnFile::remove_object`] prunes the removed object's
    /// owned string-stack entry so the positional object-to-string linkage
    /// stays consistent.
    ///
    /// Invariant asserted: removing the first of two string-owning objects drops
    /// its string, and after a round-trip the surviving object re-links to the
    /// surviving string.
    #[test]
    fn remove_object_prunes_owned_string() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 2);
        write_object(&mut bytes, ObjectFixture::sample(5, 1, 2, 17)); // owns string[0]
        write_object(&mut bytes, ObjectFixture::sample(6, 3, 4, 18)); // owns string[1]
        write_save_data(&mut bytes, 1, 6, &[], 0);
        write_string(&mut bytes, b"HELLO");
        write_string(&mut bytes, b"WORLD");
        let mut jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");
        check!(jn.strings().len() == 2);

        let removed = jn.remove_object(0).expect("object 0 should exist");
        check!(removed.object_type() == 5);
        check!(jn.strings().len() == 1);
        check!(jn.strings()[0].value() == "WORLD");

        let reparsed = JnFile::from_bytes(jn.to_bytes()).expect("re-parse should succeed");
        check!(reparsed.objects().len() == 1);
        check!(reparsed.objects()[0].object_type() == 6);
        check!(reparsed.strings().len() == 1);
        check!(reparsed.strings()[0].value() == "WORLD");
        check!(reparsed.objects()[0].string_index() == Some(0));
    }

    /// Unit under test: [`JnFile::remove_object`] with an out-of-range index.
    ///
    /// Invariant asserted: it returns `None` and leaves the layer unchanged.
    #[test]
    fn remove_object_out_of_range_returns_none() {
        let mut bytes = base_background();
        write_u16(&mut bytes, 1);
        write_object(&mut bytes, ObjectFixture::sample(5, 1, 2, 0));
        write_save_data(&mut bytes, 1, 6, &[], 0);
        let mut jn = JnFile::from_bytes(bytes).expect("JN parse should succeed");

        check!(jn.remove_object(1).is_none());
        check!(jn.remove_object(99).is_none());
        check!(jn.objects().len() == 1);
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
