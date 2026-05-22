//! Episode 1 world-map screen handler backed by `MAP.JN1`.
//!
//! Renders a static background viewport into the game area using the
//! `MAP.JN1` background layer and `JILL.DMA` map-code-to-tile lookup.  Entity
//! behavior (player movement, checkpoint objects, scrolling) is deferred to
//! the gameplay epic; this screen only draws the visible background and
//! returns to the start menu when Escape is pressed.

use crate::screens::jn_object_layer::render_jn_object_layer;
use crate::status_bar::game_area_blit;
use openjill_core::layout::{BLOCK_SIZE_I, GAME_AREA_H, GAME_AREA_W};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{JnFile, JnReadError};

/// Map code reserved as the transparent/empty cell.
///
/// `JILL.DMA` carries an entry for map code 0, but rendering treats it as a
/// no-op sentinel so the empty "sky" cells in `MAP.JN1` do not emit blits.
const TRANSPARENT_MAP_CODE: u16 = 0;

/// Initial viewport X offset in pixels for the map screen.
///
/// Pinned at the top-left of the background layer; the gameplay epic adds
/// player-driven scrolling.
const MAP_VIEWPORT_X: i32 = 0;

/// Initial viewport Y offset in pixels for the map screen.
const MAP_VIEWPORT_Y: i32 = 0;

/// Episode 1 world-map screen handler.
///
/// Owns a parsed copy of `MAP.JN1` plus the raw bytes used to produce it so
/// [`ScreenHandler::map_jn_bytes`] can round-trip the file content for save
/// and restore operations, together with a copy of `JILL.DMA` for map-code
/// to tile resolution.
pub struct MapScreen {
    /// Parsed `MAP.JN1` for background and object data.
    jn: JnFile,
    /// Raw `MAP.JN1` bytes preserved for [`ScreenHandler::map_jn_bytes`].
    jn_bytes: Vec<u8>,
    /// Parsed `JILL.DMA` lookup used to resolve map codes to tile metadata.
    dma: DmaFile,
    /// Current viewport X offset in pixels (OpenJill sign convention: a
    /// negative value scrolls the view right by `|offset|`).
    viewport_x: i32,
    /// Current viewport Y offset in pixels.
    viewport_y: i32,
}

impl MapScreen {
    /// Creates the map screen from already-parsed `MAP.JN1` data, the raw
    /// bytes that produced it, and a parsed [`DmaFile`].
    ///
    /// `jn_bytes` should be the same bytes that were fed to [`JnFile::parse`]
    /// when producing `jn`; it is retained verbatim so
    /// [`ScreenHandler::map_jn_bytes`] can hand the file content back to the
    /// orchestrator across screen transitions.
    pub fn new(jn: JnFile, jn_bytes: Vec<u8>, dma: DmaFile) -> Self {
        Self {
            jn,
            jn_bytes,
            dma,
            viewport_x: MAP_VIEWPORT_X,
            viewport_y: MAP_VIEWPORT_Y,
        }
    }

    /// Builds a [`MapScreen`] by parsing the supplied `MAP.JN1` bytes.
    ///
    /// Returns the underlying [`JnReadError`] when parsing fails.
    pub fn from_bytes(bytes: Vec<u8>, dma: DmaFile) -> Result<Self, JnReadError> {
        let jn = JnFile::from_bytes(bytes.clone())?;
        Ok(Self::new(jn, bytes, dma))
    }

    /// Processes the active input set and returns the resulting transition.
    fn process_input(&mut self, input: &ActiveInput) -> Option<ScreenTransition> {
        if input.contains(&InputCommand::Pause) {
            return Some(ScreenTransition::StartMenu);
        }
        None
    }

    /// Renders the static background viewport into the game area.
    fn render_background(&self) -> Vec<RenderCommand> {
        render_map_background(&self.jn, &self.dma, self.viewport_x, self.viewport_y)
    }

    /// Renders the static object layer for the world map.
    ///
    /// Mirrors the Java reference's `AbstractObjectJillLevel`
    /// `drawObject` pass for `MAP.JN1`: the type 0 player record placed
    /// at world (96, 40) draws as the forward-facing stand pose so Jill
    /// is visible the moment the screen appears.  Animation, gravity,
    /// checkpoint touch dispatch, doors, lifts, and the rest of the
    /// object entity loop are deferred to a follow-up; see issue #85.
    fn render_objects(&self) -> Vec<RenderCommand> {
        render_jn_object_layer(&self.jn, self.viewport_x, self.viewport_y)
    }
}

impl ScreenHandler for MapScreen {
    /// Advances the map screen by one tick: handles Escape, renders the
    /// background, and overlays the static JN object layer.
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        let transition = self.process_input(input);
        let mut commands = self.render_background();
        commands.extend(self.render_objects());
        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }

    /// Returns the raw `MAP.JN1` bytes preserved at construction.
    fn map_jn_bytes(&self) -> Option<Vec<u8>> {
        Some(self.jn_bytes.clone())
    }
}

/// Renders a `MAP.JN1` background viewport into the game area.
///
/// Walks the tile cells that overlap the visible viewport, looks each map
/// code up in `JILL.DMA`, and emits one [`RenderCommand::Blit`] per non-empty
/// cell.  Map code 0 (the transparent sentinel) is skipped, and map codes
/// without a DMA entry are silently ignored.  Output blit coordinates are
/// offset by the game-area origin (`GAME_AREA_X`, `GAME_AREA_Y`); tiles whose
/// rect lies entirely outside the game-area window are culled here.  Each
/// emitted blit carries the game-area [`openjill_core::ClipRect`] (set by
/// [`crate::status_bar::game_area_blit`]) so partial-overlap edge tiles are
/// clipped pixel-perfectly to the game area at render time, without bleeding
/// into the surrounding status-bar frame.
///
/// `offset_x` and `offset_y` follow OpenJill sign convention: a **negative**
/// value shifts the source image left/up by `|offset|` pixels, revealing
/// world content at world position `(-offset_x, -offset_y)`.
pub fn render_map_background(
    jn: &JnFile,
    dma: &DmaFile,
    offset_x: i32,
    offset_y: i32,
) -> Vec<RenderCommand> {
    // World coordinate of the top-left corner of the viewport.
    let world_x = -offset_x;
    let world_y = -offset_y;

    // Tile coordinate of the first (possibly partially visible) column/row.
    let start_tile_x = div_floor(world_x, BLOCK_SIZE_I);
    let start_tile_y = div_floor(world_y, BLOCK_SIZE_I);

    // Sub-tile pixel offset: how many pixels of the first tile are hidden.
    let sub_x = world_x.rem_euclid(BLOCK_SIZE_I);
    let sub_y = world_y.rem_euclid(BLOCK_SIZE_I);

    // Number of tile columns/rows needed to fill the game area plus one extra
    // for partially clipped edge tiles on each side.
    let tiles_x = (GAME_AREA_W as i32) / BLOCK_SIZE_I + 2;
    let tiles_y = (GAME_AREA_H as i32) / BLOCK_SIZE_I + 2;

    let game_area_w = GAME_AREA_W as i32;
    let game_area_h = GAME_AREA_H as i32;

    let mut commands = Vec::new();
    for row in 0..tiles_y {
        for col in 0..tiles_x {
            let tile_x = start_tile_x + col;
            let tile_y = start_tile_y + row;

            // Reject negative tile coordinates before casting to `usize`.
            if tile_x < 0 || tile_y < 0 {
                continue;
            }

            let Some(map_code) = jn.background().map_code(tile_x as usize, tile_y as usize) else {
                continue;
            };

            if map_code == TRANSPARENT_MAP_CODE {
                continue;
            }

            let Some(entry) = dma.get_by_map_code(map_code) else {
                continue;
            };

            let game_x = col * BLOCK_SIZE_I - sub_x;
            let game_y = row * BLOCK_SIZE_I - sub_y;

            // Cull tiles whose rect lies entirely outside the game-area
            // window; partial-overlap tiles still pass through because the
            // renderer cannot clip them per-command.
            if game_x + BLOCK_SIZE_I <= 0
                || game_x >= game_area_w
                || game_y + BLOCK_SIZE_I <= 0
                || game_y >= game_area_h
            {
                continue;
            }

            commands.push(game_area_blit(
                entry.tileset(),
                u16::from(entry.tile()),
                game_x,
                game_y,
                false,
            ));
        }
    }
    commands
}

/// Computes signed floor division, equivalent to `floor(value / divisor)`.
///
/// Standard Rust integer division truncates toward zero; this helper corrects
/// that for negative numerators so tile coordinates derived from negative
/// world offsets are accurate.
fn div_floor(value: i32, divisor: i32) -> i32 {
    let quotient = value / divisor;
    let remainder = value % divisor;
    if remainder != 0 && (remainder < 0) != (divisor < 0) {
        quotient - 1
    } else {
        quotient
    }
}

#[cfg(test)]
mod tests {
    use super::{MapScreen, render_map_background};
    use openjill_core::layout::{BLOCK_SIZE_I, GAME_AREA_X, GAME_AREA_Y};
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{
        ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition,
    };
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;

    /// Minimal valid JN byte count: 128×64 background (u16 each) + u16
    /// object count + 70-byte save-data block.
    const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;

    /// Builds a synthetic JN byte buffer where cell `(cell_x, cell_y)` holds
    /// `map_code` and all other cells are zero.  Mirrors the row-major
    /// `x * BACKGROUND_HEIGHT + y` indexing used by the parser.
    fn jn_bytes_with_cell(cell_x: usize, cell_y: usize, map_code: u16) -> Vec<u8> {
        const BACKGROUND_HEIGHT: usize = 64;
        let mut bytes = vec![0u8; JN_MIN_BYTES];
        let cell_index = cell_x * BACKGROUND_HEIGHT + cell_y;
        let byte_offset = cell_index * 2;
        bytes[byte_offset..byte_offset + 2].copy_from_slice(&map_code.to_le_bytes());
        bytes
    }

    /// Builds a synthetic `JILL.DMA` byte buffer with a single entry.
    fn dma_bytes_single(map_code: u16, tile: u8, tileset_with_flags: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&map_code.to_le_bytes());
        bytes.push(tile);
        bytes.push(tileset_with_flags);
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.push(1); // name_len
        bytes.push(b'A');
        bytes
    }

    /// Builds an empty `DmaFile`.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Per-object record size in the JN object layer (mirrors
    /// `openjill_data::jn::parse_object`'s field widths).
    const JN_OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a synthetic MAP.JN1 byte buffer containing one type-0 (player)
    /// object record at world position `(world_x, world_y)` with a 16x32
    /// bounding box.  The background layer remains all-zero; the save block
    /// also remains all-zero so the JN parser accepts it.
    fn jn_bytes_with_player(world_x: u16, world_y: u16) -> Vec<u8> {
        const BACKGROUND_BYTES: usize = 128 * 64 * 2;
        const SAVE_BLOCK_BYTES: usize = 70;
        let mut bytes = vec![0u8; BACKGROUND_BYTES + 2 + JN_OBJECT_RECORD_BYTES + SAVE_BLOCK_BYTES];

        // Object count = 1.
        let count_off = BACKGROUND_BYTES;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());

        // Object record: type 0, position, 16x32 bounding box.  All other
        // fields are left at zero.  Field layout follows
        // `openjill_data::jn::parse_object`: object_type(u8) + x(u16) +
        // y(u16) + x_speed(i16) + y_speed(i16) + width(u16) + height(u16).
        let record_off = count_off + 2;
        bytes[record_off] = 0;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&world_x.to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&world_y.to_le_bytes());
        bytes[record_off + 9..record_off + 11].copy_from_slice(&16u16.to_le_bytes());
        bytes[record_off + 11..record_off + 13].copy_from_slice(&32u16.to_le_bytes());

        bytes
    }

    /// Unit under test: `MapScreen::from_bytes` accepts an all-zero MAP.JN1
    /// fixture (8192 zero map codes + zero objects + 70-byte save data).
    ///
    /// Preconditions: synthetic zero-filled JN bytes of the minimum valid
    /// length; empty DMA.
    ///
    /// Invariants asserted: `from_bytes` returns `Ok` and a subsequent idle
    /// tick produces no transition.
    #[test]
    fn from_bytes_parses_zero_map_jn() {
        let bytes = vec![0u8; JN_MIN_BYTES];
        let mut screen =
            MapScreen::from_bytes(bytes, empty_dma()).expect("synthetic zero MAP.JN1 should parse");
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(result.transition.is_none(), "idle tick must not transition");
    }

    /// Unit under test: `render_map_background` for a non-zero map code that
    /// resolves to a `(tileset, tile)` pair through `JILL.DMA`.
    ///
    /// Preconditions: cell (0, 0) holds map code 0x0001; DMA has one entry
    /// mapping code 1 to tileset 1 / tile 0.
    ///
    /// Invariants asserted: exactly one `Blit` command is emitted, with the
    /// expected tileset/tile and game-area-origin-offset coordinates `(80, 16)`.
    #[test]
    fn non_zero_map_code_produces_blit_with_dma_tileset_tile() {
        let jn = JnFile::from_bytes(jn_bytes_with_cell(0, 0, 1)).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 1)).expect("DMA should parse");
        let commands = render_map_background(&jn, &dma, 0, 0);

        let blits: Vec<&RenderCommand> = commands
            .iter()
            .filter(|c| matches!(c, RenderCommand::Blit { .. }))
            .collect();
        assert_eq!(blits.len(), 1, "exactly one Blit expected for one cell");
        match blits[0] {
            RenderCommand::Blit {
                tileset,
                tile,
                x,
                y,
                opaque,
                clip: _,
            } => {
                assert_eq!(*tileset, 1);
                assert_eq!(*tile, 0);
                assert_eq!(*x, GAME_AREA_X);
                assert_eq!(*y, GAME_AREA_Y);
                assert!(!*opaque);
            }
            _ => unreachable!(),
        }
    }

    /// Unit under test: `render_map_background` does not emit a blit for a
    /// non-zero map code located outside the visible viewport.
    ///
    /// Preconditions: cell (50, 50) holds map code 1 (far outside the
    /// game-area viewport of ~16×12 cells); DMA maps code 1.
    ///
    /// Invariants asserted: no `Blit` command is produced.
    #[test]
    fn map_codes_outside_viewport_emit_no_blit() {
        let jn = JnFile::from_bytes(jn_bytes_with_cell(50, 50, 1)).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 1)).expect("DMA should parse");
        let commands = render_map_background(&jn, &dma, 0, 0);
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, RenderCommand::Blit { .. })),
            "cells outside the viewport must not produce blits"
        );
    }

    /// Unit under test: `render_map_background` skips cells whose map code is
    /// 0 even when `JILL.DMA` defines an entry for code 0.
    ///
    /// Preconditions: every cell is 0; DMA has an entry mapping code 0 to a
    /// valid `(tileset, tile)` pair.
    ///
    /// Invariants asserted: zero `Blit` commands are emitted, confirming that
    /// the transparent sentinel short-circuits before the DMA lookup.
    #[test]
    fn map_code_zero_skipped_even_when_dma_resolves_it() {
        let jn = JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(0, 5, 3)).expect("DMA should parse");
        let commands = render_map_background(&jn, &dma, 0, 0);
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, RenderCommand::Blit { .. })),
            "map code 0 must be treated as the transparent sentinel"
        );
    }

    /// Unit under test: `MapScreen::tick` returns
    /// `ScreenTransition::StartMenu` when Escape (Pause) is pressed.
    #[test]
    fn escape_returns_to_start_menu() {
        let bytes = vec![0u8; JN_MIN_BYTES];
        let mut screen =
            MapScreen::from_bytes(bytes, empty_dma()).expect("synthetic zero MAP.JN1 should parse");
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Pause);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::StartMenu));
    }

    /// Unit under test: an idle tick on a synthetic MAP.JN1 carrying a single
    /// type-0 player object emits a `Blit` of the player tileset at the
    /// framebuffer position the world-to-screen translation predicts.
    ///
    /// Preconditions: synthetic MAP.JN1 with one type-0 object at world
    /// (96, 40), 16x32; empty DMA; viewport defaults to (0, 0).
    ///
    /// Invariants asserted: exactly one player-tileset `Blit` is present in
    /// the tick's command list (the background contributes none because all
    /// map codes are zero), and its framebuffer position matches
    /// `(GAME_AREA_X + 96, GAME_AREA_Y + 40)`.
    #[test]
    fn player_object_emits_blit_at_world_to_screen_position() {
        const PLAYER_TILESET: u8 = 8;
        let bytes = jn_bytes_with_player(96, 40);
        let mut screen = MapScreen::from_bytes(bytes, empty_dma())
            .expect("synthetic MAP.JN1 with one player should parse");
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());

        let player_blits: Vec<&RenderCommand> = result
            .commands
            .iter()
            .filter(|c| match c {
                RenderCommand::Blit { tileset, .. } => *tileset == PLAYER_TILESET,
                _ => false,
            })
            .collect();

        assert_eq!(
            player_blits.len(),
            1,
            "exactly one player-tileset Blit expected"
        );
        match player_blits[0] {
            RenderCommand::Blit { x, y, .. } => {
                assert_eq!(*x, GAME_AREA_X + 96);
                assert_eq!(*y, GAME_AREA_Y + 40);
            }
            _ => unreachable!(),
        }
    }

    /// Unit under test: `MapScreen::map_jn_bytes` returns the original bytes
    /// passed to `from_bytes`, supporting `putCurrentLevelInFileMemory`
    /// semantics during screen transitions.
    #[test]
    fn map_jn_bytes_round_trip_preserves_source_bytes() {
        let mut bytes = vec![0u8; JN_MIN_BYTES];
        bytes[0..2].copy_from_slice(&0x0042u16.to_le_bytes());
        let screen = MapScreen::from_bytes(bytes.clone(), empty_dma())
            .expect("synthetic MAP.JN1 should parse");
        assert_eq!(screen.map_jn_bytes(), Some(bytes));
    }

    /// Unit under test: at viewport (0, 0) the first emitted blit lands at the
    /// game-area origin and successive cells advance by `BLOCK_SIZE_I` along
    /// each axis.
    ///
    /// Preconditions: cells (0, 0) and (1, 0) and (0, 1) all hold map code 1
    /// with a single matching DMA entry.
    ///
    /// Invariants asserted: the three emitted blits sit at framebuffer
    /// coordinates `(GAME_AREA_X, GAME_AREA_Y)`, `(GAME_AREA_X + 16,
    /// GAME_AREA_Y)`, and `(GAME_AREA_X, GAME_AREA_Y + 16)` respectively.
    #[test]
    fn adjacent_cells_advance_by_block_size() {
        const BACKGROUND_HEIGHT: usize = 64;
        let mut bytes = vec![0u8; JN_MIN_BYTES];
        for &(cx, cy) in &[(0usize, 0usize), (1, 0), (0, 1)] {
            let cell_index = cx * BACKGROUND_HEIGHT + cy;
            let off = cell_index * 2;
            bytes[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        let jn = JnFile::from_bytes(bytes).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 1)).expect("DMA should parse");
        let commands = render_map_background(&jn, &dma, 0, 0);

        let mut positions: Vec<(i32, i32)> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Blit { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect();
        positions.sort();

        let mut expected = vec![
            (GAME_AREA_X, GAME_AREA_Y),
            (GAME_AREA_X + BLOCK_SIZE_I, GAME_AREA_Y),
            (GAME_AREA_X, GAME_AREA_Y + BLOCK_SIZE_I),
        ];
        expected.sort();
        assert_eq!(positions, expected);
    }

    /// Unit under test: `render_map_background` with a non-zero viewport
    /// offset culls tiles whose rect lies entirely past the game-area right
    /// edge while still emitting a blit for a partially clipped left-edge
    /// tile.
    ///
    /// Preconditions: viewport offset `(-100, 0)` (world x = 100, sub_x = 4,
    /// `start_tile_x = 6`).  Cells (6, 0) and (21, 0) both hold map code 1.
    /// Cell (6, 0) maps to col 0 with `game_x = -4` (partial overlap on the
    /// left, kept). Cell (21, 0) maps to col 15 with `game_x = 236`, past
    /// `GAME_AREA_W = 232` (culled).
    ///
    /// Invariants asserted: exactly one `Blit` is emitted, at framebuffer
    /// coordinates `(GAME_AREA_X - 4, GAME_AREA_Y)`; no blit reaches the
    /// `game_x >= 232` half-open right edge.
    #[test]
    fn non_zero_offset_culls_tiles_past_right_edge() {
        const BACKGROUND_HEIGHT: usize = 64;
        let mut bytes = vec![0u8; JN_MIN_BYTES];
        for &(cx, cy) in &[(6usize, 0usize), (21, 0)] {
            let cell_index = cx * BACKGROUND_HEIGHT + cy;
            let off = cell_index * 2;
            bytes[off..off + 2].copy_from_slice(&1u16.to_le_bytes());
        }
        let jn = JnFile::from_bytes(bytes).expect("JN should parse");
        let dma = DmaFile::from_bytes(dma_bytes_single(1, 0, 1)).expect("DMA should parse");

        let commands = render_map_background(&jn, &dma, -100, 0);
        let blits: Vec<(i32, i32)> = commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::Blit { x, y, .. } => Some((*x, *y)),
                _ => None,
            })
            .collect();

        assert_eq!(blits.len(), 1, "right-edge tile must be culled");
        assert_eq!(blits[0], (GAME_AREA_X - 4, GAME_AREA_Y));
    }
}
