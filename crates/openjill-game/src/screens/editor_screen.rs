//! In-game level editor screen - the `Ctrl+E` title-screen cheat (epic #210).
//!
//! v1 scope: a tile cursor moves over an editable board, the selected tile
//! cycles through the `JILL.DMA` palette, the selected tile is painted at the
//! cursor, and `Escape` returns to the start menu. The board starts blank
//! ([`JnFile::blank`]).
//!
//! Controls (mapped onto the existing [`InputCommand`] vocabulary):
//!
//! | Input | Action |
//! |-------|--------|
//! | Arrow keys | Move the tile cursor (one tile per press) |
//! | Tab / Backspace | Next / previous palette tile |
//! | Space / Shift | Paint the selected tile at the cursor |
//! | Escape | Return to the start menu |
//!
//! The original DOS editor's wider key map (`K` pick, `L`/`S` load/save, `O`
//! object mode, ...) needs raw-key input that [`ScreenHandler::tick`] does not
//! yet carry; those land in later epic-#210 sub-issues.

use crate::screens::intro_background::render_intro_background;
use openjill_core::layout::{BLOCK_SIZE_I, GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{BACKGROUND_HEIGHT, BACKGROUND_WIDTH, JnFile};

/// Number of whole tile columns the game area shows.
const VISIBLE_TILES_X: usize = GAME_AREA_W as usize / BLOCK_SIZE_I as usize;
/// Number of whole tile rows the game area shows.
const VISIBLE_TILES_Y: usize = GAME_AREA_H as usize / BLOCK_SIZE_I as usize;
/// Palette index of the cursor outline (bright EGA yellow).
const CURSOR_COLOR: u8 = 14;
/// Cursor outline thickness in framebuffer pixels.
const CURSOR_THICKNESS: i32 = 1;

/// The in-game level editor screen.
pub struct EditorScreen {
    /// The board being edited.
    board: JnFile,
    /// Tile metadata used both for palette tiles and background rendering.
    dma: DmaFile,
    /// Cursor column in board-tile coordinates (`0..BACKGROUND_WIDTH`).
    cursor_x: usize,
    /// Cursor row in board-tile coordinates (`0..BACKGROUND_HEIGHT`).
    cursor_y: usize,
    /// Top-left visible tile column (camera), kept so the cursor stays on-screen.
    camera_x: usize,
    /// Top-left visible tile row (camera).
    camera_y: usize,
    /// Index into `dma.entries()` of the tile painted by [`Self::place_tile`];
    /// `None` only when the DMA palette is empty.
    selected_entry: Option<usize>,
    /// Input set held during the previous tick, for per-command rising-edge
    /// detection (so one key press performs exactly one action).
    prev_input: ActiveInput,
}

impl EditorScreen {
    /// Creates the editor over `board` with the `dma` palette, cursor at the
    /// top-left and the first palette tile selected (if any).
    pub fn new(board: JnFile, dma: DmaFile) -> Self {
        let selected_entry = (!dma.entries().is_empty()).then_some(0);
        Self {
            board,
            dma,
            cursor_x: 0,
            cursor_y: 0,
            camera_x: 0,
            camera_y: 0,
            selected_entry,
            prev_input: ActiveInput::new(),
        }
    }

    /// Moves the cursor by `(dx, dy)` tiles, clamped to the board, then scrolls
    /// the camera so the cursor stays visible.
    fn move_cursor(&mut self, dx: isize, dy: isize) {
        self.cursor_x = clamp_step(self.cursor_x, dx, BACKGROUND_WIDTH);
        self.cursor_y = clamp_step(self.cursor_y, dy, BACKGROUND_HEIGHT);
        self.scroll_into_view();
    }

    /// Scrolls the camera the minimum amount to keep the cursor within the
    /// visible window.
    fn scroll_into_view(&mut self) {
        if self.cursor_x < self.camera_x {
            self.camera_x = self.cursor_x;
        } else if self.cursor_x >= self.camera_x + VISIBLE_TILES_X {
            self.camera_x = self.cursor_x + 1 - VISIBLE_TILES_X;
        }
        if self.cursor_y < self.camera_y {
            self.camera_y = self.cursor_y;
        } else if self.cursor_y >= self.camera_y + VISIBLE_TILES_Y {
            self.camera_y = self.cursor_y + 1 - VISIBLE_TILES_Y;
        }
    }

    /// Cycles the selected palette tile by `delta` entries (wrapping). A no-op
    /// when the DMA palette is empty.
    fn cycle_tile(&mut self, delta: isize) {
        let len = self.dma.entries().len();
        if len == 0 {
            return;
        }
        let current = self.selected_entry.unwrap_or(0) as isize;
        self.selected_entry = Some((current + delta).rem_euclid(len as isize) as usize);
    }

    /// Paints the selected palette tile's map code at the cursor. A no-op when
    /// no tile is selected (empty palette).
    fn place_tile(&mut self) {
        let Some(index) = self.selected_entry else {
            return;
        };
        let Some(entry) = self.dma.entries().get(index) else {
            return;
        };
        let code = entry.map_code();
        self.board
            .set_background_code(self.cursor_x, self.cursor_y, code);
    }

    /// Processes the rising-edge input set, returning a transition when the
    /// player exits.
    fn process_input(&mut self, input: &ActiveInput) -> Option<ScreenTransition> {
        let pressed: ActiveInput = input.difference(&self.prev_input).copied().collect();
        self.prev_input = input.clone();
        if pressed.is_empty() {
            return None;
        }

        if pressed.contains(&InputCommand::Pause) {
            return Some(ScreenTransition::StartMenu);
        }

        if pressed.contains(&InputCommand::MoveLeft) {
            self.move_cursor(-1, 0);
        }
        if pressed.contains(&InputCommand::MoveRight) {
            self.move_cursor(1, 0);
        }
        if pressed.contains(&InputCommand::Up) {
            self.move_cursor(0, -1);
        }
        if pressed.contains(&InputCommand::Duck) {
            self.move_cursor(0, 1);
        }
        if pressed.contains(&InputCommand::NextInventory) {
            self.cycle_tile(1);
        }
        if pressed.contains(&InputCommand::PrevInventory) {
            self.cycle_tile(-1);
        }
        if pressed.contains(&InputCommand::Jump) {
            self.place_tile();
        }

        None
    }

    /// Renders the board viewport plus the cursor outline.
    fn render(&self) -> Vec<RenderCommand> {
        let offset_x = -(self.camera_x as i32 * BLOCK_SIZE_I);
        let offset_y = -(self.camera_y as i32 * BLOCK_SIZE_I);
        let mut commands = render_intro_background(&self.board, &self.dma, offset_x, offset_y);
        commands.extend(self.cursor_outline());
        commands
    }

    /// Builds the four-edge cursor outline at the cursor's on-screen tile.
    ///
    /// `scroll_into_view` guarantees the cursor is within the visible window, so
    /// the subtraction below never underflows and the rect stays in the game
    /// area.
    fn cursor_outline(&self) -> [RenderCommand; 4] {
        let col = (self.cursor_x - self.camera_x) as i32;
        let row = (self.cursor_y - self.camera_y) as i32;
        let left = GAME_AREA_X + col * BLOCK_SIZE_I;
        let top = GAME_AREA_Y + row * BLOCK_SIZE_I;
        let span = BLOCK_SIZE_I as u32;
        let thickness = CURSOR_THICKNESS as u32;
        [
            RenderCommand::FillRect {
                x: left,
                y: top,
                width: span,
                height: thickness,
                color: CURSOR_COLOR,
            },
            RenderCommand::FillRect {
                x: left,
                y: top + BLOCK_SIZE_I - CURSOR_THICKNESS,
                width: span,
                height: thickness,
                color: CURSOR_COLOR,
            },
            RenderCommand::FillRect {
                x: left,
                y: top,
                width: thickness,
                height: span,
                color: CURSOR_COLOR,
            },
            RenderCommand::FillRect {
                x: left + BLOCK_SIZE_I - CURSOR_THICKNESS,
                y: top,
                width: thickness,
                height: span,
                color: CURSOR_COLOR,
            },
        ]
    }
}

impl ScreenHandler for EditorScreen {
    /// Advances the editor one tick: applies input, then renders the board and
    /// cursor.
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        let transition = self.process_input(input);
        let commands = self.render();
        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }
}

/// Adds `delta` to `value`, clamping the result to `0..len`.
fn clamp_step(value: usize, delta: isize, len: usize) -> usize {
    (value as isize + delta).clamp(0, len as isize - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::{EditorScreen, VISIBLE_TILES_X};
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{ActiveInput, InputCommand, ScreenHandler, ScreenTransition};
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;

    /// A DMA file with a single entry whose map code is `code`, for
    /// palette/placement tests.
    fn dma_with_map_code(code: u16) -> DmaFile {
        // DMA record layout (per the parser): map_code u16, tile u8,
        // tileset+flags u8, flags u16, name_len u8, then `name_len` name bytes.
        let name = b"TILE";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&code.to_le_bytes()); // map_code
        bytes.push(3); // tile
        bytes.push(7); // tileset (+flag bits)
        bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        bytes.push(name.len() as u8); // name_len
        bytes.extend_from_slice(name); // name
        DmaFile::from_bytes(bytes).expect("single-entry DMA should parse")
    }

    /// Empty DMA palette.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Presses `cmd` for one tick (rising edge), then releases it, returning the
    /// transition from the press tick.
    fn press(screen: &mut EditorScreen, cmd: InputCommand) -> Option<ScreenTransition> {
        let mut input = ActiveInput::new();
        input.insert(cmd);
        let transition = screen.tick(&input, &mut RuntimeState::new()).transition;
        screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        transition
    }

    /// Unit under test: `Escape` exits the editor to the start menu.
    #[test]
    fn escape_returns_to_start_menu() {
        let mut screen = EditorScreen::new(JnFile::blank(), empty_dma());
        assert_eq!(
            press(&mut screen, InputCommand::Pause),
            Some(ScreenTransition::StartMenu)
        );
    }

    /// Unit under test: `Space` (Jump) paints the selected palette tile's map
    /// code at the cursor, surviving a `to_bytes` round-trip.
    #[test]
    fn space_paints_selected_tile_at_cursor() {
        let mut screen = EditorScreen::new(JnFile::blank(), dma_with_map_code(0x123));
        // Move one tile right and one down, then paint.
        press(&mut screen, InputCommand::MoveRight);
        press(&mut screen, InputCommand::Duck);
        press(&mut screen, InputCommand::Jump);

        let reparsed = JnFile::from_bytes(screen.board.to_bytes()).expect("board must round-trip");
        assert_eq!(reparsed.background().map_code(1, 1), Some(0x123));
        // Untouched cells stay blank.
        assert_eq!(reparsed.background().map_code(0, 0), Some(0));
    }

    /// Unit under test: cursor movement clamps at the board's left/top edge.
    #[test]
    fn cursor_clamps_at_top_left() {
        let mut screen = EditorScreen::new(JnFile::blank(), empty_dma());
        press(&mut screen, InputCommand::MoveLeft);
        press(&mut screen, InputCommand::Up);
        assert_eq!((screen.cursor_x, screen.cursor_y), (0, 0));
    }

    /// Unit under test: moving past the visible window scrolls the camera so the
    /// cursor stays on-screen.
    #[test]
    fn camera_follows_cursor_past_the_visible_window() {
        let mut screen = EditorScreen::new(JnFile::blank(), empty_dma());
        for _ in 0..VISIBLE_TILES_X {
            press(&mut screen, InputCommand::MoveRight);
        }
        // Cursor advanced one tile beyond the initial window; camera scrolled by
        // exactly one to keep it on the right edge.
        assert_eq!(screen.cursor_x, VISIBLE_TILES_X);
        assert_eq!(screen.camera_x, 1);
        assert!(screen.cursor_x < screen.camera_x + VISIBLE_TILES_X);
    }

    /// Unit under test: tile cycling wraps and an empty palette never selects.
    #[test]
    fn palette_cycling_wraps_and_handles_empty() {
        let mut full = EditorScreen::new(JnFile::blank(), dma_with_map_code(0x55));
        assert_eq!(full.selected_entry, Some(0));
        press(&mut full, InputCommand::NextInventory);
        assert_eq!(full.selected_entry, Some(0)); // single entry wraps to itself

        let mut empty = EditorScreen::new(JnFile::blank(), empty_dma());
        assert_eq!(empty.selected_entry, None);
        press(&mut empty, InputCommand::NextInventory);
        assert_eq!(empty.selected_entry, None);
    }
}
