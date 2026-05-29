//! Parser for `JILL1.CFG` high scores, save slots, and setup/configuration data.

use crate::{ByteReader, ByteReaderError};
use std::error::Error;
use std::fmt::{Display, Formatter};

/// Number of high-score name slots in `JILL1.CFG`.
const HIGH_SCORE_COUNT: usize = 10;
/// Fixed byte width of each high-score name slot.
const HIGH_SCORE_NAME_LEN: usize = 10;
/// Byte size of the unused hole between high-score names and high-score values.
const HIGH_SCORE_HOLE_LEN: usize = 20;
/// Number of save-name slots in `JILL1.CFG`.
const SAVE_SLOT_COUNT: usize = 6;
/// Fixed byte width of each save-name slot.
const SAVE_NAME_LEN: usize = 12;
/// Lowest byte value treated as printable by OpenJill (`> 31`).
const PRINTABLE_ASCII_MIN_EXCLUSIVE: u8 = 31;
/// Highest byte value treated as printable by OpenJill (`< 128`).
const PRINTABLE_ASCII_MAX_EXCLUSIVE: u8 = 128;
/// Byte offset of the i32-LE high-score value block (after names + hole).
const HIGH_SCORE_VALUE_OFFSET: usize = HIGH_SCORE_COUNT * HIGH_SCORE_NAME_LEN + HIGH_SCORE_HOLE_LEN;
/// Byte offset of the save-name block (after names + hole + scores).
const SAVE_NAME_OFFSET: usize = HIGH_SCORE_VALUE_OFFSET + HIGH_SCORE_COUNT * 4;
/// Byte length of the trailing setup block (11 × i16: setup flag, joystick
/// flag, six calibration values, display mode, music flag, sound flag).
const SETUP_BLOCK_LEN: usize = 11 * 2;
/// Total byte length of a `JILL1.CFG` file.
pub const FILE_LEN: usize = SAVE_NAME_OFFSET + SAVE_SLOT_COUNT * SAVE_NAME_LEN + SETUP_BLOCK_LEN;
/// Padding byte written after a name, mirroring Java
/// `CfgFileImpl.FILE_SPACE_FILLER = '\0'`.
const CFG_NAME_PAD: u8 = 0;

/// One high-score entry parsed from `JILL1.CFG`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfgHighScore {
    /// Player name filtered using OpenJill's printable-ASCII rules.
    name: String,
    /// Signed score value read as `i32le`.
    score: i32,
}

impl CfgHighScore {
    /// Returns the high-score name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the signed score value.
    pub fn score(&self) -> i32 {
        self.score
    }
}

/// One save-slot entry parsed from `JILL1.CFG`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfgSaveSlot {
    /// Slot display name filtered using OpenJill's printable-ASCII rules.
    name: String,
    /// Episode-specific save-file path stem (`{prefix}SAVE.{index}`).
    save_game_file: String,
    /// Episode-specific save-map path stem (`{prefix}SAVEM.{index}`).
    save_map_file: String,
}

impl CfgSaveSlot {
    /// Returns the slot display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the slot's save-game filename stem.
    pub fn save_game_file(&self) -> &str {
        &self.save_game_file
    }

    /// Returns the slot's save-map filename stem.
    pub fn save_map_file(&self) -> &str {
        &self.save_map_file
    }
}

/// Joystick calibration values parsed from the setup block.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfgJoystickCalibration {
    /// Left X-axis calibration bound.
    left_x: i16,
    /// Center X-axis calibration value.
    center_x: i16,
    /// Right X-axis calibration bound.
    right_x: i16,
    /// Lower Y-axis calibration bound.
    lower_y: i16,
    /// Center Y-axis calibration value.
    center_y: i16,
    /// Upper Y-axis calibration bound.
    upper_y: i16,
}

impl CfgJoystickCalibration {
    /// Returns the left X-axis calibration bound.
    pub fn left_x(&self) -> i16 {
        self.left_x
    }

    /// Returns the center X-axis calibration value.
    pub fn center_x(&self) -> i16 {
        self.center_x
    }

    /// Returns the right X-axis calibration bound.
    pub fn right_x(&self) -> i16 {
        self.right_x
    }

    /// Returns the lower Y-axis calibration bound.
    pub fn lower_y(&self) -> i16 {
        self.lower_y
    }

    /// Returns the center Y-axis calibration value.
    pub fn center_y(&self) -> i16 {
        self.center_y
    }

    /// Returns the upper Y-axis calibration bound.
    pub fn upper_y(&self) -> i16 {
        self.upper_y
    }
}

/// Common setup/configuration block parsed from `JILL1.CFG`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfgSetup {
    /// Whether setup is required (`raw_setup_flag == 1` in OpenJill).
    setup_required: bool,
    /// Whether joystick support is enabled (`raw_joystick_flag != 0`).
    joystick_enabled: bool,
    /// Joystick calibration values.
    joystick_calibration: CfgJoystickCalibration,
    /// Display mode value (`1 = CGA`, `2 = EGA`, `4 = VGA` in Jill).
    display_mode: i16,
    /// Whether music playback is enabled (`raw_music_flag != 0`).
    music_enabled: bool,
    /// Whether digital sound playback is enabled (`raw_sound_flag != 0`).
    sound_enabled: bool,
}

impl CfgSetup {
    /// Returns whether setup is required.
    pub fn setup_required(&self) -> bool {
        self.setup_required
    }

    /// Returns whether joystick support is enabled.
    pub fn joystick_enabled(&self) -> bool {
        self.joystick_enabled
    }

    /// Returns parsed joystick calibration values.
    pub fn joystick_calibration(&self) -> &CfgJoystickCalibration {
        &self.joystick_calibration
    }

    /// Returns the parsed display mode value.
    pub fn display_mode(&self) -> i16 {
        self.display_mode
    }

    /// Returns whether music playback is enabled.
    pub fn music_enabled(&self) -> bool {
        self.music_enabled
    }

    /// Returns whether digital sound playback is enabled.
    pub fn sound_enabled(&self) -> bool {
        self.sound_enabled
    }
}

/// Parsed `JILL1.CFG` content for one episode prefix (for example `JN1`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CfgFile {
    /// Parsed high-score table entries in source order.
    high_scores: Vec<CfgHighScore>,
    /// Parsed save-slot entries in source order.
    save_slots: Vec<CfgSaveSlot>,
    /// Parsed setup/configuration block.
    setup: CfgSetup,
    /// Verbatim source bytes, used as the source of truth for [`Self::to_bytes`].
    ///
    /// Captured from the reader by [`Self::parse`] (so every constructor retains
    /// it).  Mutators ([`Self::add_high_score`], [`Self::set_save_slot_name`])
    /// patch the relevant region here so the rest of the file (the high-score
    /// hole, the setup/joystick block, any trailing bytes) round-trips
    /// byte-for-byte and stays readable by the original DOS game.
    raw: Vec<u8>,
}

impl CfgFile {
    /// Parses `JILL1.CFG` from a reader for the given save-file `prefix`.
    ///
    /// Retains the reader's full backing bytes so [`Self::to_bytes`] can
    /// round-trip them after mutation, regardless of which constructor was used.
    pub fn parse(reader: &mut ByteReader, prefix: &str) -> Result<Self, CfgReadError> {
        let raw = reader.as_bytes().to_vec();
        let mut high_score_names = Vec::with_capacity(HIGH_SCORE_COUNT);
        for entry_index in 0..HIGH_SCORE_COUNT {
            high_score_names.push(read_high_score_name(reader, entry_index)?);
        }

        let skip_offset = reader.offset();
        reader
            .skip(HIGH_SCORE_HOLE_LEN)
            .map_err(|source| CfgReadError {
                field: "high_score_hole",
                entry_index: None,
                offset: error_offset(&source, skip_offset),
                source,
            })?;

        let mut high_scores = Vec::with_capacity(HIGH_SCORE_COUNT);
        for (entry_index, name) in high_score_names.iter().enumerate() {
            let score = read_i32(reader, "high_score", entry_index)?;
            high_scores.push(CfgHighScore {
                name: name.clone(),
                score,
            });
        }

        let mut save_slots = Vec::with_capacity(SAVE_SLOT_COUNT);
        for entry_index in 0..SAVE_SLOT_COUNT {
            let name = read_save_name(reader, entry_index)?;
            save_slots.push(CfgSaveSlot {
                name,
                save_game_file: format!("{prefix}SAVE.{entry_index}"),
                save_map_file: format!("{prefix}SAVEM.{entry_index}"),
            });
        }

        let setup_required = read_i16(reader, "setup_flag", None)? == 1;
        let joystick_enabled = read_i16(reader, "joystick_flag", None)? != 0;

        let joystick_calibration = CfgJoystickCalibration {
            left_x: read_i16(reader, "joystick_left_x", None)?,
            center_x: read_i16(reader, "joystick_center_x", None)?,
            right_x: read_i16(reader, "joystick_right_x", None)?,
            lower_y: read_i16(reader, "joystick_left_y", None)?,
            center_y: read_i16(reader, "joystick_center_y", None)?,
            upper_y: read_i16(reader, "joystick_right_y", None)?,
        };

        let display_mode = read_i16(reader, "display_mode", None)?;
        let music_enabled = read_i16(reader, "music_flag", None)? != 0;
        let sound_enabled = read_i16(reader, "sound_flag", None)? != 0;

        Ok(Self {
            high_scores,
            save_slots,
            setup: CfgSetup {
                setup_required,
                joystick_enabled,
                joystick_calibration,
                display_mode,
                music_enabled,
                sound_enabled,
            },
            raw,
        })
    }

    /// Parses a `CfgFile` directly from in-memory bytes using `prefix`.
    ///
    /// Retains the source bytes so [`Self::to_bytes`] can round-trip them.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>, prefix: &str) -> Result<Self, CfgReadError> {
        let mut reader = ByteReader::from_bytes(bytes);
        Self::parse(&mut reader, prefix)
    }

    /// Builds an empty default config (no high scores, no save names, zeroed
    /// setup) for `prefix` - used to seed a fresh writable config when no
    /// original `JILL1.CFG` is available.
    pub fn empty(prefix: &str) -> Self {
        Self::from_bytes(vec![0u8; FILE_LEN], prefix)
            .expect("a zero-filled buffer of FILE_LEN always parses")
    }

    /// Serialises the config back to bytes: byte-identical to the source when
    /// unmodified, and patched in place after mutations.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.raw.clone()
    }

    /// Inserts a high score keeping the table sorted descending and capped at
    /// [`HIGH_SCORE_COUNT`], then patches the name/score regions of the source
    /// bytes.  Mirrors Java `CfgFileImpl.addNewHighScore`.
    pub fn add_high_score(&mut self, name: &str, score: i32) {
        let position = self
            .high_scores
            .iter()
            .position(|entry| score > entry.score)
            .unwrap_or(self.high_scores.len());
        self.high_scores.insert(
            position,
            CfgHighScore {
                // Normalise to what actually persists (printable, slot width)
                // so the in-memory value matches a later `to_bytes`/re-parse.
                name: normalize_name(name, HIGH_SCORE_NAME_LEN),
                score,
            },
        );
        self.high_scores.truncate(HIGH_SCORE_COUNT);
        self.rewrite_high_scores();
    }

    /// Sets a save-slot display name and patches its name region in the source
    /// bytes.  Out-of-range indices are ignored.
    pub fn set_save_slot_name(&mut self, index: usize, name: &str) {
        let normalized = normalize_name(name, SAVE_NAME_LEN);
        let Some(slot) = self.save_slots.get_mut(index) else {
            return;
        };
        let offset = SAVE_NAME_OFFSET + index * SAVE_NAME_LEN;
        write_name_slot(&mut self.raw, offset, &normalized, SAVE_NAME_LEN);
        slot.name = normalized;
    }

    /// Re-emits every high-score name and score into the source bytes from the
    /// current in-memory table.
    fn rewrite_high_scores(&mut self) {
        let entries: Vec<(String, i32)> = self
            .high_scores
            .iter()
            .map(|entry| (entry.name.clone(), entry.score))
            .collect();
        for (index, (name, score)) in entries.iter().enumerate() {
            write_name_slot(
                &mut self.raw,
                index * HIGH_SCORE_NAME_LEN,
                name,
                HIGH_SCORE_NAME_LEN,
            );
            let score_offset = HIGH_SCORE_VALUE_OFFSET + index * 4;
            if let Some(slot) = self.raw.get_mut(score_offset..score_offset + 4) {
                slot.copy_from_slice(&score.to_le_bytes());
            }
        }
    }

    /// Returns parsed high-score entries.
    pub fn high_scores(&self) -> &[CfgHighScore] {
        &self.high_scores
    }

    /// Returns parsed save-slot entries.
    pub fn save_slots(&self) -> &[CfgSaveSlot] {
        &self.save_slots
    }

    /// Returns parsed setup/configuration data.
    pub fn setup(&self) -> &CfgSetup {
        &self.setup
    }
}

/// Error returned when parsing a `JILL1.CFG` file fails.
#[derive(Debug, Eq, PartialEq)]
pub struct CfgReadError {
    /// Name of the field being parsed when the failure occurred.
    pub field: &'static str,
    /// Optional table index for high-score/save-slot scoped failures.
    pub entry_index: Option<usize>,
    /// Source offset associated with the parse failure.
    pub offset: usize,
    /// Underlying byte-reader failure.
    source: ByteReaderError,
}

impl Display for CfgReadError {
    /// Formats the error including field name, entry index (if any), and offset.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(entry_index) = self.entry_index {
            write!(
                f,
                "failed to parse CFG field '{}' for entry {} at offset {}: {}",
                self.field, entry_index, self.offset, self.source
            )
        } else {
            write!(
                f,
                "failed to parse CFG field '{}' at offset {}: {}",
                self.field, self.offset, self.source
            )
        }
    }
}

impl Error for CfgReadError {
    /// Returns the underlying byte-reader failure as the source error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}

/// Reads one high-score name slot (10 bytes) with printable-ASCII filtering.
fn read_high_score_name(
    reader: &mut ByteReader,
    entry_index: usize,
) -> Result<String, CfgReadError> {
    let mut name = String::with_capacity(HIGH_SCORE_NAME_LEN);
    for _ in 0..HIGH_SCORE_NAME_LEN {
        let byte = read_u8(reader, "high_score_name", Some(entry_index))?;
        if is_openjill_printable_ascii(byte) {
            name.push(char::from(byte));
        }
    }

    Ok(name)
}

/// Reads one save-slot name (12 bytes), stopping at first non-printable byte.
fn read_save_name(reader: &mut ByteReader, entry_index: usize) -> Result<String, CfgReadError> {
    let mut name = String::with_capacity(SAVE_NAME_LEN);

    for index_char in 0..SAVE_NAME_LEN {
        let byte = read_u8(reader, "save_name", Some(entry_index))?;
        if is_openjill_printable_ascii(byte) {
            name.push(char::from(byte));
        } else {
            let remaining = SAVE_NAME_LEN - index_char - 1;
            if remaining > 0 {
                let skip_offset = reader.offset();
                reader.skip(remaining).map_err(|source| CfgReadError {
                    field: "save_name_skip_remaining",
                    entry_index: Some(entry_index),
                    offset: error_offset(&source, skip_offset),
                    source,
                })?;
            }
            break;
        }
    }

    Ok(name)
}

/// Returns whether `byte` is considered printable by OpenJill CFG parsing.
fn is_openjill_printable_ascii(byte: u8) -> bool {
    byte > PRINTABLE_ASCII_MIN_EXCLUSIVE && byte < PRINTABLE_ASCII_MAX_EXCLUSIVE
}

/// Normalises a name to exactly what a fixed-width slot persists: the printable
/// bytes of `name`, truncated to `len`.  Used so an in-memory name matches the
/// value that survives [`CfgFile::to_bytes`] / re-parse.
fn normalize_name(name: &str, len: usize) -> String {
    name.bytes()
        .filter(|&b| is_openjill_printable_ascii(b))
        .take(len)
        .map(char::from)
        .collect()
}

/// Writes a fixed-width name slot at `offset`: the printable bytes of `name`
/// (truncated to `len`) followed by [`CFG_NAME_PAD`] padding, mirroring Java
/// `CfgFileImpl.writeName*InFile`.  No-op if the slot is out of range.
fn write_name_slot(raw: &mut [u8], offset: usize, name: &str, len: usize) {
    let Some(slot) = raw.get_mut(offset..offset + len) else {
        return;
    };
    slot.fill(CFG_NAME_PAD);
    for (dst, byte) in slot
        .iter_mut()
        .zip(name.bytes().filter(|&b| is_openjill_printable_ascii(b)))
    {
        *dst = byte;
    }
}

/// Reads one `u8` field and wraps reader errors with CFG parse context.
fn read_u8(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
) -> Result<u8, CfgReadError> {
    let fallback_offset = reader.offset();
    reader.read_u8().map_err(|source| CfgReadError {
        field,
        entry_index,
        offset: error_offset(&source, fallback_offset),
        source,
    })
}

/// Reads one `i16le` field and wraps reader errors with CFG parse context.
fn read_i16(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: Option<usize>,
) -> Result<i16, CfgReadError> {
    let fallback_offset = reader.offset();
    reader.read_i16_le().map_err(|source| CfgReadError {
        field,
        entry_index,
        offset: error_offset(&source, fallback_offset),
        source,
    })
}

/// Reads one `i32le` field and wraps reader errors with CFG parse context.
fn read_i32(
    reader: &mut ByteReader,
    field: &'static str,
    entry_index: usize,
) -> Result<i32, CfgReadError> {
    let fallback_offset = reader.offset();
    reader.read_i32_le().map_err(|source| CfgReadError {
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
    use super::{
        CfgFile, CfgReadError, HIGH_SCORE_COUNT, HIGH_SCORE_HOLE_LEN, HIGH_SCORE_NAME_LEN,
        SAVE_NAME_LEN, SAVE_SLOT_COUNT,
    };
    use crate::ByteReaderError;
    use assert2::check;

    /// Unit under test: `CfgFile::parse` high-score name/score extraction.
    ///
    /// Preconditions: a synthetic 254-byte CFG fixture where each 10-byte
    /// high-score slot includes printable bytes mixed with filtered control or
    /// high-bit bytes, followed by ten signed `i32le` score values.
    ///
    /// Invariants asserted: names are filtered using OpenJill printable-ASCII
    /// rules while preserving slot order and score pairing, and all ten signed
    /// high-score values are parsed exactly.
    #[test]
    fn parses_high_score_names_and_scores_from_synthetic_fixture() {
        let mut bytes = cfg_fixture_template();

        write_high_score_name_slot(
            &mut bytes,
            0,
            &[b'A', b'B', 0, b'C', 31, b'D', 128, b'E', b'!', b'Z'],
        );
        write_high_score_name_slot(&mut bytes, 1, b"PLAYERTWO!");
        write_high_score_name_slot(&mut bytes, 2, b"THIRD-----");
        write_high_score_name_slot(&mut bytes, 3, b"FOURTH----");
        write_high_score_name_slot(&mut bytes, 4, b"FIFTH-----");
        write_high_score_name_slot(&mut bytes, 5, b"SIXTH-----");
        write_high_score_name_slot(&mut bytes, 6, b"SEVENTH---");
        write_high_score_name_slot(&mut bytes, 7, b"EIGHTH----");
        write_high_score_name_slot(&mut bytes, 8, b"NINTH-----");
        write_high_score_name_slot(&mut bytes, 9, b"TENTH-----");

        let scores = [
            12_345,
            -1,
            0,
            i32::MIN,
            i32::MAX,
            7,
            -99,
            543_210,
            -3_210,
            42,
        ];
        write_high_scores(&mut bytes, &scores);

        let cfg = CfgFile::from_bytes(bytes, "JN1").expect("CFG parse should succeed");

        check!(cfg.high_scores().len() == HIGH_SCORE_COUNT);
        check!(cfg.high_scores()[0].name() == "ABCDE!Z");
        for (index, score) in scores.iter().enumerate() {
            check!(cfg.high_scores()[index].score() == *score);
        }
    }

    /// Unit under test: `CfgFile::parse` save-slot parsing and file metadata.
    ///
    /// Preconditions: a synthetic 254-byte CFG fixture with six 12-byte save
    /// slots, including early NUL terminators that force skip-ahead behavior.
    ///
    /// Invariants asserted: each save-slot name respects OpenJill's
    /// first-non-printable termination rule, and each slot is paired with the
    /// expected episode-specific `JN1SAVE.{index}` and `JN1SAVEM.{index}`
    /// metadata stems.
    #[test]
    fn parses_save_slot_names_and_jn1_metadata_from_synthetic_fixture() {
        let mut bytes = cfg_fixture_template();

        write_save_name_slot(
            &mut bytes,
            0,
            &[
                b'S', b'l', b'o', b't', b'0', 0, b'X', b'X', b'X', b'X', b'X', b'X',
            ],
        );
        write_save_name_slot(&mut bytes, 1, b"SECOND SLOT!");
        write_save_name_slot(
            &mut bytes,
            2,
            &[
                b'T', b'H', b'I', b'R', b'D', 31, b'Y', b'Y', b'Y', b'Y', b'Y', b'Y',
            ],
        );
        write_save_name_slot(&mut bytes, 3, b"FOURTH_SLOT!");
        write_save_name_slot(&mut bytes, 4, b"FIFTH-SLOT!!");
        write_save_name_slot(&mut bytes, 5, b"SIXTH SLOT!!");

        let cfg = CfgFile::from_bytes(bytes, "JN1").expect("CFG parse should succeed");

        check!(cfg.save_slots().len() == SAVE_SLOT_COUNT);
        check!(cfg.save_slots()[0].name() == "Slot0");
        check!(cfg.save_slots()[1].name() == "SECOND SLOT!");
        check!(cfg.save_slots()[2].name() == "THIRD");
        for (index, slot) in cfg.save_slots().iter().enumerate() {
            check!(slot.save_game_file() == format!("JN1SAVE.{index}"));
            check!(slot.save_map_file() == format!("JN1SAVEM.{index}"));
        }
    }

    /// Unit under test: `CfgFile::parse` setup/config block extraction.
    ///
    /// Preconditions: a synthetic 254-byte CFG fixture whose final ten `i16le`
    /// fields encode setup, joystick, joystick calibration, display mode,
    /// music, and sound values.
    ///
    /// Invariants asserted: setup and joystick booleans follow Java semantics
    /// (`setup == 1`, others nonzero), calibration values are preserved exactly,
    /// and display/music/sound fields decode to the expected values.
    #[test]
    fn parses_setup_and_joystick_display_music_sound_from_synthetic_fixture() {
        let mut bytes = cfg_fixture_template();

        write_setup_block(&mut bytes, &[1, 2, -100, 0, 200, -300, 400, 500, 4, 1, 0]);

        let cfg = CfgFile::from_bytes(bytes, "JN1").expect("CFG parse should succeed");
        let setup = cfg.setup();

        check!(setup.setup_required());
        check!(setup.joystick_enabled());
        check!(setup.joystick_calibration().left_x() == -100);
        check!(setup.joystick_calibration().center_x() == 0);
        check!(setup.joystick_calibration().right_x() == 200);
        check!(setup.joystick_calibration().lower_y() == -300);
        check!(setup.joystick_calibration().center_y() == 400);
        check!(setup.joystick_calibration().upper_y() == 500);
        check!(setup.display_mode() == 4);
        check!(setup.music_enabled());
        check!(!setup.sound_enabled());
    }

    /// Unit under test: `CfgReadError` offset reporting for truncated score data.
    ///
    /// Preconditions: a fixture truncated one byte into the first high-score
    /// `i32le`, so the score read fails immediately after names and hole.
    ///
    /// Invariants asserted: parsing fails with `field == "high_score"`,
    /// `entry_index == Some(0)`, and an offset pointing at the first score
    /// byte (`120`) where the `i32le` read starts.
    #[test]
    fn includes_failing_offset_when_high_score_table_is_truncated() {
        let bytes = vec![0; HIGH_SCORE_COUNT * 10 + HIGH_SCORE_HOLE_LEN + 1];

        check!(
            let Err(err) = CfgFile::from_bytes(bytes, "JN1")
                && err == CfgReadError {
                    field: "high_score",
                    entry_index: Some(0),
                    offset: 120,
                    source: ByteReaderError::UnexpectedEof {
                        operation: "read signed 32-bit little-endian integer",
                        offset: 120,
                        requested: 4,
                        len: 121,
                    },
                }
        );
    }

    /// Returns an all-zero synthetic CFG fixture with the exact expected file
    /// size (254 bytes), suitable for selective field writes in tests.
    fn cfg_fixture_template() -> Vec<u8> {
        vec![0; 254]
    }

    /// Writes a raw 10-byte high-score name slot into the fixture.
    fn write_high_score_name_slot(bytes: &mut [u8], index: usize, value: &[u8]) {
        let start = index * 10;
        bytes[start..start + 10].copy_from_slice(value);
    }

    /// Writes ten signed `i32le` high-score values after names and hole bytes.
    fn write_high_scores(bytes: &mut [u8], scores: &[i32; HIGH_SCORE_COUNT]) {
        let start = (HIGH_SCORE_COUNT * 10) + HIGH_SCORE_HOLE_LEN;
        for (index, score) in scores.iter().enumerate() {
            let pos = start + (index * 4);
            bytes[pos..pos + 4].copy_from_slice(&score.to_le_bytes());
        }
    }

    /// Writes one raw 12-byte save-name slot into the fixture.
    fn write_save_name_slot(bytes: &mut [u8], index: usize, value: &[u8]) {
        let start =
            (HIGH_SCORE_COUNT * 10) + HIGH_SCORE_HOLE_LEN + (HIGH_SCORE_COUNT * 4) + (index * 12);
        bytes[start..start + 12].copy_from_slice(value);
    }

    /// Writes the setup/config block values as signed `i16le` fields.
    fn write_setup_block(bytes: &mut [u8], values: &[i16; 11]) {
        let start = (HIGH_SCORE_COUNT * 10)
            + HIGH_SCORE_HOLE_LEN
            + (HIGH_SCORE_COUNT * 4)
            + (SAVE_SLOT_COUNT * 12);
        for (index, value) in values.iter().enumerate() {
            let pos = start + (index * 2);
            bytes[pos..pos + 2].copy_from_slice(&value.to_le_bytes());
        }
    }

    /// Builds a populated 254-byte fixture: one named/scored high score, one
    /// named save slot, and a non-zero setup block.
    fn populated_fixture() -> Vec<u8> {
        let mut bytes = cfg_fixture_template();
        write_high_score_name_slot(&mut bytes, 0, b"ACE\0\0\0\0\0\0\0");
        let mut scores = [0i32; HIGH_SCORE_COUNT];
        scores[0] = 5000;
        write_high_scores(&mut bytes, &scores);
        write_save_name_slot(&mut bytes, 0, b"SLOT0\0\0\0\0\0\0\0");
        write_setup_block(&mut bytes, &[1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1]);
        bytes
    }

    /// Unit under test: [`CfgFile::to_bytes`].
    ///
    /// Invariants asserted: an unmodified `CfgFile` round-trips its source
    /// bytes byte-for-byte (the hole and setup block are preserved verbatim).
    #[test]
    fn to_bytes_round_trips_source_bytes_unmodified() {
        let bytes = populated_fixture();
        let cfg = CfgFile::from_bytes(bytes.clone(), "JN1").expect("fixture parses");
        check!(cfg.to_bytes() == bytes);
    }

    /// Unit under test: [`CfgFile::add_high_score`].
    ///
    /// Invariants asserted: a higher score sorts to the top and the table stays
    /// capped at `HIGH_SCORE_COUNT`; re-parsing `to_bytes` reflects the new
    /// top entry, while the setup block stays byte-identical.
    #[test]
    fn add_high_score_inserts_sorted_and_persists() {
        let bytes = populated_fixture();
        let mut cfg = CfgFile::from_bytes(bytes.clone(), "JN1").expect("fixture parses");
        cfg.add_high_score("WIN", 9000);

        check!(cfg.high_scores().len() == HIGH_SCORE_COUNT);
        check!(cfg.high_scores()[0].name() == "WIN");
        check!(cfg.high_scores()[0].score() == 9000);

        let reparsed = CfgFile::from_bytes(cfg.to_bytes(), "JN1").expect("re-parse");
        check!(reparsed.high_scores()[0].name() == "WIN");
        check!(reparsed.high_scores()[0].score() == 9000);
        // The setup block (last 22 bytes) is untouched by the score edit.
        check!(cfg.to_bytes()[232..] == bytes[232..]);
    }

    /// Unit under test: [`CfgFile::set_save_slot_name`].
    ///
    /// Invariants asserted: the slot name updates in memory and persists
    /// through `to_bytes` / re-parse.
    #[test]
    fn set_save_slot_name_persists() {
        let bytes = populated_fixture();
        let mut cfg = CfgFile::from_bytes(bytes, "JN1").expect("fixture parses");
        cfg.set_save_slot_name(1, "MYSAVE");

        check!(cfg.save_slots()[1].name() == "MYSAVE");
        let reparsed = CfgFile::from_bytes(cfg.to_bytes(), "JN1").expect("re-parse");
        check!(reparsed.save_slots()[1].name() == "MYSAVE");
    }

    /// Unit under test: name normalisation in the mutators.
    ///
    /// Invariants asserted: an over-long name is truncated to the slot width in
    /// memory, so the value reported before persistence equals the value after
    /// `to_bytes` / re-parse (no silent divergence).
    #[test]
    fn mutated_names_match_after_reparse() {
        let mut cfg = CfgFile::from_bytes(populated_fixture(), "JN1").expect("fixture parses");
        cfg.add_high_score("ABCDEFGHIJKLMNOP", 9999); // 16 chars > 10-byte slot
        cfg.set_save_slot_name(2, "VERYLONGSAVENAME"); // 16 chars > 12-byte slot

        let hs_name = cfg.high_scores()[0].name().to_string();
        let save_name = cfg.save_slots()[2].name().to_string();
        check!(hs_name.len() == HIGH_SCORE_NAME_LEN);
        check!(save_name.len() == SAVE_NAME_LEN);

        let reparsed = CfgFile::from_bytes(cfg.to_bytes(), "JN1").expect("re-parse");
        check!(reparsed.high_scores()[0].name() == hs_name);
        check!(reparsed.save_slots()[2].name() == save_name);
    }
}
