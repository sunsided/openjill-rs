//! In-game level editor screen - the `Ctrl+E` title-screen cheat (epic #210).
//!
//! v1 scope: a tile cursor moves over an editable board, the selected tile
//! cycles through the `JILL.DMA` palette, the selected tile is painted at the
//! cursor, and `Escape` returns to the start menu. The board starts blank
//! ([`JnFile::blank`]).
//!
//! Controls (movement/paint via [`InputCommand`]; letter commands via the
//! per-tick typed-character channel [`RuntimeState::text_input`]):
//!
//! | Input | Action |
//! |-------|--------|
//! | Arrow keys | Move the tile cursor (one tile per press) |
//! | Tab / Backspace | Next / previous palette tile |
//! | Space / Shift | Paint the selected tile at the cursor |
//! | `K` | Pick the tile under the cursor as the selected tile |
//! | `H` | Flood-fill the cursor row with the selected tile |
//! | `Z` / `N` | Clear to a new blank board |
//! | Escape | Return to the start menu |
//!
//! The remaining DOS editor commands (`L`/`S` load/save, `O` object mode,
//! `Enter` load-by-name, Tab continuous-draw, Shift half-screen jumps) land in
//! later epic-#210 sub-issues.

use crate::screens::intro_background::render_intro_background;
use openjill_core::layout::{BLOCK_SIZE_I, GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{BACKGROUND_HEIGHT, BACKGROUND_MAP_CODE_MASK, BACKGROUND_WIDTH, JnFile};

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

    /// Picks the tile under the cursor as the selected palette tile (the `K`
    /// command): looks up the DMA entry whose map code matches the cell. A no-op
    /// when the cell's code has no DMA entry (e.g. a blank `0` cell).
    fn pick_tile(&mut self) {
        let code = self
            .board
            .background()
            .map_code(self.cursor_x, self.cursor_y)
            .unwrap_or(0);
        if let Some(entry) = self.dma.get_by_map_code(code) {
            self.selected_entry = Some(entry.index());
        }
    }

    /// Fills the cursor's row horizontally (the `H` command): replaces the run
    /// of cells equal to the cursor cell's current code with the selected tile,
    /// extending left and right while the code matches. A no-op when nothing is
    /// selected or the cell already holds the selected code.
    fn flood_fill_row(&mut self) {
        let Some(index) = self.selected_entry else {
            return;
        };
        let Some(entry) = self.dma.entries().get(index) else {
            return;
        };
        let new_code = entry.map_code() & BACKGROUND_MAP_CODE_MASK;
        let row = self.cursor_y;
        let target = self
            .board
            .background()
            .map_code(self.cursor_x, row)
            .unwrap_or(0);
        if target == new_code {
            return;
        }
        // Right from the cursor.
        let mut x = self.cursor_x;
        while self.board.background().map_code(x, row) == Some(target) {
            self.board.set_background_code(x, row, new_code);
            x += 1;
            if x >= BACKGROUND_WIDTH {
                break;
            }
        }
        // Left from the cursor.
        let mut x = self.cursor_x;
        while x > 0 {
            x -= 1;
            if self.board.background().map_code(x, row) != Some(target) {
                break;
            }
            self.board.set_background_code(x, row, new_code);
        }
    }

    /// Replaces the board with a fresh blank one and resets the cursor/camera
    /// (the `Z` clear / `N` new-board commands).
    fn clear_board(&mut self) {
        self.board = JnFile::blank();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.camera_x = 0;
        self.camera_y = 0;
    }

    /// Applies the editor's letter-key commands from this tick's typed
    /// characters ([`RuntimeState::text_input`]): `K` picks the tile under the
    /// cursor, `H` flood-fills the cursor row, `Z`/`N` clear to a new blank
    /// board. Case-insensitive. Returns
    /// `true` when at least one command ran (so the caller can suppress paint -
    /// an uppercase command is typed with Shift, which also maps to the paint
    /// key).
    fn handle_text_commands(&mut self, typed: &[char]) -> bool {
        let mut handled = false;
        for ch in typed {
            match ch.to_ascii_lowercase() {
                'k' => self.pick_tile(),
                'h' => self.flood_fill_row(),
                'z' | 'n' => self.clear_board(),
                _ => continue,
            }
            handled = true;
        }
        handled
    }

    /// Processes the rising-edge input set, returning a transition when the
    /// player exits.
    ///
    /// `suppress_paint` skips the paint action for this tick; the caller sets it
    /// when a letter command ran, because an uppercase command is typed with
    /// Shift, which also maps to the paint key ([`InputCommand::Jump`]).
    fn process_input(
        &mut self,
        input: &ActiveInput,
        suppress_paint: bool,
    ) -> Option<ScreenTransition> {
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
        if !suppress_paint && pressed.contains(&InputCommand::Jump) {
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
    fn tick(&mut self, input: &ActiveInput, state: &mut RuntimeState) -> TickResult {
        // Letter-key commands (typed characters) run first and take precedence
        // over paint: an uppercase command (e.g. `K`) is typed with Shift, which
        // also maps to the paint key, so a paint here would clobber the cell that
        // `K` is meant to pick. When a command ran, paint is suppressed.
        let ran_command = self.handle_text_commands(&state.text_input);
        let transition = self.process_input(input, ran_command);
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
    use super::{BACKGROUND_WIDTH, EditorScreen, VISIBLE_TILES_X};
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

    /// A DMA file with one entry per map code in `codes` (index = position).
    fn dma_with_codes(codes: &[u16]) -> DmaFile {
        let mut bytes = Vec::new();
        for (i, &code) in codes.iter().enumerate() {
            bytes.extend_from_slice(&code.to_le_bytes()); // map_code
            bytes.push(i as u8); // tile
            bytes.push(7); // tileset (+flags)
            bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
            bytes.push(1); // name_len
            bytes.push(b'T'); // name
        }
        DmaFile::from_bytes(bytes).expect("multi-entry DMA should parse")
    }

    /// Empty DMA palette.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Feeds one typed character to the editor for a single tick (the editor's
    /// letter-key command channel), with no held `InputCommand`s.
    fn type_char(screen: &mut EditorScreen, ch: char) {
        let mut state = RuntimeState::new();
        state.text_input = vec![ch];
        screen.tick(&ActiveInput::new(), &mut state);
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

    /// Unit under test: the `K` command picks the tile under the cursor.
    ///
    /// Paints entry 0 at (0,0) and entry 1 at (1,0), leaving entry 1 selected;
    /// moving back over (0,0) and typing `K` re-selects entry 0.
    #[test]
    fn k_picks_the_tile_under_the_cursor() {
        let mut screen = EditorScreen::new(JnFile::blank(), dma_with_codes(&[0x0A, 0x0B]));
        press(&mut screen, InputCommand::Jump); // paint entry 0 (0x0A) at (0,0)
        press(&mut screen, InputCommand::MoveRight);
        press(&mut screen, InputCommand::NextInventory); // select entry 1 (0x0B)
        press(&mut screen, InputCommand::Jump); // paint entry 1 at (1,0)
        assert_eq!(screen.selected_entry, Some(1));

        press(&mut screen, InputCommand::MoveLeft); // back to (0,0)
        type_char(&mut screen, 'k');
        assert_eq!(screen.selected_entry, Some(0));
    }

    /// Unit under test: uppercase `K` (Shift+K) picks without painting.
    ///
    /// Regression: Shift maps to `Jump` (paint), so `Shift+K` arrives as typed
    /// `K` plus a paint command in the same tick. Letter commands must run first
    /// and suppress the paint, so `K` picks the cell's existing tile instead of
    /// the paint clobbering it with the selected tile.
    #[test]
    fn uppercase_k_picks_without_painting() {
        let mut screen = EditorScreen::new(JnFile::blank(), dma_with_codes(&[0x0A, 0x0B]));
        press(&mut screen, InputCommand::Jump); // paint entry 0 (0x0A) at (0,0)
        press(&mut screen, InputCommand::NextInventory); // select entry 1 (0x0B)
        assert_eq!(screen.selected_entry, Some(1));

        // Shift+K at (0,0): typed 'K' plus Jump (Shift) on the same tick.
        let mut state = RuntimeState::new();
        state.text_input = vec!['K'];
        let mut shifted = ActiveInput::new();
        shifted.insert(InputCommand::Jump);
        screen.tick(&shifted, &mut state);

        assert_eq!(
            screen.selected_entry,
            Some(0),
            "K picks the cell's own tile"
        );
        let reparsed = JnFile::from_bytes(screen.board.to_bytes()).expect("board round-trips");
        assert_eq!(
            reparsed.background().map_code(0, 0),
            Some(0x0A),
            "paint must be suppressed, leaving the cell unchanged"
        );
    }

    /// Unit under test: the `H` command flood-fills the cursor's row.
    ///
    /// On a blank board the whole row shares code 0, so filling at any cell
    /// replaces the entire row with the selected tile; other rows are untouched.
    #[test]
    fn h_flood_fills_the_cursor_row() {
        let mut screen = EditorScreen::new(JnFile::blank(), dma_with_codes(&[0x0A]));
        for _ in 0..3 {
            press(&mut screen, InputCommand::MoveRight);
        }
        for _ in 0..2 {
            press(&mut screen, InputCommand::Duck); // row 2
        }
        type_char(&mut screen, 'h');

        let reparsed = JnFile::from_bytes(screen.board.to_bytes()).expect("board round-trips");
        for x in 0..BACKGROUND_WIDTH {
            assert_eq!(
                reparsed.background().map_code(x, 2),
                Some(0x0A),
                "row cell {x} must be filled"
            );
        }
        assert_eq!(
            reparsed.background().map_code(0, 1),
            Some(0),
            "other rows untouched"
        );
    }

    /// Unit under test: the `N`/`Z` command clears to a new blank board and
    /// resets the cursor.
    #[test]
    fn n_clears_the_board_and_resets_cursor() {
        let mut screen = EditorScreen::new(JnFile::blank(), dma_with_codes(&[0x0A]));
        press(&mut screen, InputCommand::MoveRight);
        press(&mut screen, InputCommand::Jump); // paint at (1,0)
        type_char(&mut screen, 'n');

        assert_eq!((screen.cursor_x, screen.cursor_y), (0, 0));
        let reparsed = JnFile::from_bytes(screen.board.to_bytes()).expect("board round-trips");
        assert!(reparsed.background().map_codes().iter().all(|&c| c == 0));
    }
}
