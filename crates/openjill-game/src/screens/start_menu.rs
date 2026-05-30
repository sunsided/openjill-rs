//! Start menu screen handler backed by `INTRO.JN1`.
//!
//! Implements the episode-1 start menu described in
//! `StartMenuJill1Handler` and `start_menu.json` from the Java reference port.
//! The background is rendered from the INTRO.JN1 background layer at the
//! fixed viewport offset `(-1808, -864)`.  On top of the background a tileset-7
//! menu box is drawn, and within it the menu title, item list, and optional
//! overlays (info box, load-game) are rendered as `DrawText`/`FillRect`
//! commands.

use crate::screens::intro_background::render_intro_background;
use crate::screens::jn_object_layer::render_jn_object_layer;
use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::layout::{CONTROL_AREA_X, CONTROL_AREA_Y, INVENTORY_AREA_X, INVENTORY_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, FontSize, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::cfg::CfgFile;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::sha::ShaFile;
use openjill_data::vcl::VclFile;
use std::sync::LazyLock;

/// Embedded `start_menu.json` layout resource from the Java reference port.
const START_MENU_JSON: &str = include_str!("../../resources/start_menu.json");

/// Background viewport X offset from `StartMenuJill1Handler::centerScreen`.
///
/// `-(112 + 1) * 16 = -1808`.
const START_MENU_OFFSET_X: i32 = -1808;

/// Background viewport Y offset from `StartMenuJill1Handler::centerScreen`.
///
/// `-(53 + 1) * 16 = -864`.
const START_MENU_OFFSET_Y: i32 = -864;

/// Left edge of the info-box text in framebuffer pixels.
///
/// Matches `information_box.json` text origin (box `x` 92 + `offsetTextDrawX` 8).
const INFO_BOX_TEXT_X: i32 = 100;

/// Top edge of the info-box first text line in framebuffer pixels.
const INFO_BOX_TEXT_Y: i32 = 44;

/// Vertical advance per info-box text line in pixels.
///
/// Matches the SHA font row height used by [`StartMenuScreen::render_menu_text`].
const INFO_BOX_LINE_HEIGHT: i32 = 8;

/// Maximum number of info-box text lines drawn before clipping.
///
/// Mirrors `nbLineDraw` from `information_box.json` so overflow lines beyond the
/// 106-pixel textarea height are dropped rather than running past the box.
const INFO_BOX_MAX_LINES: usize = 12;

/// Palette index used for info-box text.
///
/// Matches `information_box.json` `textColor`.
const INFO_BOX_TEXT_COLOR: u8 = 7;

/// Which optional overlay is currently drawn over the base menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Overlay {
    /// No overlay active; base menu is displayed.
    None,
    /// Instructions info-box overlay (VCL entry 0 text).
    InfoBox,
    /// Load-game slot overlay.
    LoadGame,
}

/// One menu item parsed from `start_menu.json`.
#[derive(Clone, Debug)]
struct MenuItem {
    /// Display text.
    text: String,
    /// Palette color index for the item text.
    color: u8,
    /// Action value used to determine the transition/overlay on confirmation.
    value: i32,
}

/// Parsed `start_menu.json` layout used by `StartMenuScreen`.
struct MenuLayout {
    /// Menu box top-left X in framebuffer pixel coordinates.
    x: i32,
    /// Menu box top-left Y in framebuffer pixel coordinates.
    y: i32,
    /// Pixel columns per text character (from `textX` * BLOCK_SIZE in practice,
    /// but treated here as absolute pixel position of the first text column).
    text_col_px: i32,
    /// Pixel rows per text line.
    text_row_px: i32,
    /// Number of leading space characters before each item text string.
    nb_space_before: usize,
    /// Menu title settings (color + text).
    title_color: u8,
    /// Menu title text.
    title_text: String,
    /// All menu items in source order.
    items: Vec<MenuItem>,
    // --- Tile references for the menu box frame ---
    /// Tileset index for all menu box tiles.
    frame_tileset: u8,
    /// Top-right corner tile.
    right_upper: u16,
    /// Top-left corner tile.
    left_upper: u16,
    /// Bottom-right corner tile.
    right_lower: u16,
    /// Bottom-left corner tile.
    left_lower: u16,
    /// Top edge tile.
    upper_bar: u16,
    /// Bottom edge tile.
    lower_bar: u16,
    /// Right edge tile.
    right_bar: u16,
    /// Left edge tile.
    left_bar: u16,
    /// Interior fill tile.
    back_image: u16,
}

/// Number of animation frames in the menu cursor.
///
/// Mirrors `AbstractMenu.NB_CURSOR_IMAGE` from the Java reference: the
/// small SHA font carries an eight-frame diamond/spinner animation at
/// codepoints `\u{0001}..=\u{0008}` that the cursor cycles through.
const MENU_CURSOR_FRAMES: u8 = 8;

/// Palette index used for the menu cursor.
///
/// Matches `TextManager.COLOR_WHITE` in the Java reference: the cursor is
/// always drawn white regardless of the highlighted row's text color.
const MENU_CURSOR_COLOR: u8 = 7;

/// Pixel size of one menu box frame tile.
///
/// The shipped `start_menu.json` references tiles from `JILL1.SHA` tileset
/// 7, whose corner / bar / fill tiles are all 8x8.  The previous menu box
/// renderer stepped through positions at the 16-pixel block size and left
/// 8-pixel gaps between every tile; the original DOS layout draws them
/// adjacent at 8-pixel stride.
const MENU_FRAME_TILE_SIZE: i32 = 8;

/// Lazily parsed `start_menu.json` layout, loaded once on first access.
static MENU_LAYOUT: LazyLock<MenuLayout> = LazyLock::new(parse_menu_layout);

/// Full start menu screen handler.
///
/// Owns the `INTRO.JN1`, `JILL.DMA`, `JILL1.VCL`, and `JILL1.CFG` data needed
/// to render the background, menu box, overlays, and high-score panel. Data is
/// cloned from the asset cache once on construction and held for the lifetime of
/// the screen.
pub struct StartMenuScreen {
    /// Parsed `INTRO.JN1` for background rendering.
    intro: JnFile,
    /// Parsed `JILL.DMA` for map-code to tile lookup.
    dma: DmaFile,
    /// Parsed `JILL1.VCL` for the info-box overlay text.
    vcl: VclFile,
    /// Parsed `JILL1.CFG` for high scores and save-slot names.
    cfg: CfgFile,
    /// Currently selected menu item index (0-based).
    selected: usize,
    /// Active overlay rendered above the base menu.
    overlay: Overlay,
    /// Current frame in the menu cursor animation (`0..MENU_CURSOR_FRAMES`).
    ///
    /// Advanced once per tick to match `AbstractMenu.drawCursor`'s
    /// `cursorIndex++` increment in the Java reference.
    cursor_index: u8,
    /// Pre-computed Jill-portrait tile layout derived from SHA tile heights.
    ///
    /// Built once at construction by [`compute_portrait_tiles`] so the per-row
    /// vertical stacking and the per-tile bottom-anchor for row 3 stay in
    /// sync with the actual SHA tileset 24 dimensions.
    portrait_tiles: [(i32, i32, u16); 16],
    /// Highlighted save slot in the load-game overlay (`0`-based).
    load_cursor: usize,
    /// Input set held during the previous tick.  Menu input is latched to the
    /// per-command rising edge (only keys *newly* pressed this tick act), so one
    /// key press performs exactly one action: held keys do not repeat, and
    /// Escape cannot dismiss an overlay and then quit on the same hold.  Keeping
    /// the full set (rather than a single "any key" flag) lets an independent
    /// key still register while another is held.
    prev_input: ActiveInput,
}

impl StartMenuScreen {
    /// Creates the start menu screen from pre-loaded episode data.
    ///
    /// `sha` is consumed only to derive the Jill-portrait tile layout; the
    /// caller retains ownership of the file.
    pub fn new(intro: JnFile, dma: DmaFile, vcl: VclFile, cfg: CfgFile, sha: &ShaFile) -> Self {
        let portrait_tiles = compute_portrait_tiles(sha);
        Self {
            intro,
            dma,
            vcl,
            cfg,
            selected: 0,
            overlay: Overlay::None,
            cursor_index: 0,
            portrait_tiles,
            load_cursor: 0,
            prev_input: ActiveInput::new(),
        }
    }
}

impl ScreenHandler for StartMenuScreen {
    /// Renders one start-menu tick, processes input, and optionally transitions.
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        let transition = self.process_input(input);
        let commands = self.render_frame();
        self.cursor_index = (self.cursor_index + 1) % MENU_CURSOR_FRAMES;
        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }
}

impl StartMenuScreen {
    /// Processes the active input set and updates overlay/selection/transition.
    ///
    /// Menu input is latched to the **per-command rising edge**: only keys that
    /// are newly pressed this tick (held last tick = ignored) drive actions.
    /// This stops a held arrow from scrolling rapidly and stops a held Escape
    /// from dismissing an overlay and then quitting on the same press, while
    /// still letting an independent key register when another is held.
    fn process_input(&mut self, input: &ActiveInput) -> Option<ScreenTransition> {
        // Keys newly pressed this tick: the current set minus what was held last
        // tick.  All command checks below run against this rising-edge set.
        let pressed: ActiveInput = input.difference(&self.prev_input).copied().collect();
        self.prev_input = input.clone();
        if pressed.is_empty() {
            return None;
        }
        let input = &pressed;

        // Any newly pressed key dismisses the info-box overlay.
        if self.overlay == Overlay::InfoBox {
            self.overlay = Overlay::None;
            return None;
        }

        // Escape / Pause dismisses the load-game overlay, or quits the game.
        if input.contains(&InputCommand::Pause) {
            if self.overlay == Overlay::LoadGame {
                self.overlay = Overlay::None;
                return None;
            }
            return Some(ScreenTransition::Quit);
        }

        // Q key quits directly.
        if input.contains(&InputCommand::Quit) {
            return Some(ScreenTransition::Quit);
        }

        // Load-game overlay: navigate the save slots and confirm to load.
        if self.overlay == Overlay::LoadGame {
            let slot_count = self.cfg.save_slots().len().max(1);
            if input.contains(&InputCommand::ThrowItem) || input.contains(&InputCommand::Jump) {
                let slot = self.load_cursor;
                self.overlay = Overlay::None;
                return Some(ScreenTransition::PerformLoad { slot });
            }
            if input.contains(&InputCommand::Up) || input.contains(&InputCommand::PrevInventory) {
                self.load_cursor = (self.load_cursor + slot_count - 1) % slot_count;
            } else if input.contains(&InputCommand::Duck)
                || input.contains(&InputCommand::NextInventory)
            {
                self.load_cursor = (self.load_cursor + 1) % slot_count;
            }
            return None;
        }

        // Ctrl+P: play the intro level (the title-screen cheat). Checked before
        // the confirm key because Ctrl also maps to ThrowItem (confirm), so this
        // early return keeps the chord from also activating the selected item.
        if input.contains(&InputCommand::PlayIntro) {
            return Some(ScreenTransition::Level {
                file: String::from("INTRO.JN1"),
                number: 0,
            });
        }

        let layout = &*MENU_LAYOUT;
        if layout.items.is_empty() {
            return None;
        }

        // Confirm selection: ThrowItem (Ctrl) or Jump (Space / Alt).
        if input.contains(&InputCommand::ThrowItem) || input.contains(&InputCommand::Jump) {
            return self.apply_value(layout.items[self.selected].value);
        }

        // Navigate up: ArrowUp or Backspace.
        if input.contains(&InputCommand::Up) || input.contains(&InputCommand::PrevInventory) {
            let len = layout.items.len();
            self.selected = (self.selected + len - 1) % len;
            return None;
        }

        // Navigate down: ArrowDown or Tab.
        if input.contains(&InputCommand::Duck) || input.contains(&InputCommand::NextInventory) {
            self.selected = (self.selected + 1) % layout.items.len();
            return None;
        }

        None
    }

    /// Applies a confirmed menu item value, returning the resulting transition or
    /// updating the overlay state.
    fn apply_value(&mut self, value: i32) -> Option<ScreenTransition> {
        match value {
            0 => Some(ScreenTransition::Map),
            1 => {
                self.overlay = Overlay::LoadGame;
                self.load_cursor = 0;
                None
            }
            2 => Some(ScreenTransition::Story),
            3 => {
                self.overlay = Overlay::InfoBox;
                None
            }
            4 => Some(ScreenTransition::OrderingInfo),
            5 => Some(ScreenTransition::Credits),
            7 => Some(ScreenTransition::Noisemaker),
            9 => Some(ScreenTransition::Quit),
            _ => None,
        }
    }

    /// Renders all layers for one frame: background → menu box → text →
    /// high-score panel → active overlay.
    fn render_frame(&self) -> Vec<RenderCommand> {
        let mut commands = render_intro_background(
            &self.intro,
            &self.dma,
            START_MENU_OFFSET_X,
            START_MENU_OFFSET_Y,
        );
        commands.extend(render_jn_object_layer(
            &self.intro,
            START_MENU_OFFSET_X,
            START_MENU_OFFSET_Y,
        ));
        commands.extend(self.render_menu_box());
        commands.extend(self.render_menu_text());
        commands.extend(self.render_high_score_panel());
        commands.extend(render_jill_portrait(&self.portrait_tiles));
        match self.overlay {
            Overlay::InfoBox => commands.extend(self.render_info_box()),
            Overlay::LoadGame => commands.extend(self.render_load_game()),
            Overlay::None => {}
        }
        commands
    }

    /// Emits `Blit` commands for the tileset-7 menu box frame.
    ///
    /// Mirrors `AbstractStdMenu.drawPicture` from the Java reference: the box
    /// holds `items.len() + 5` total cell rows (`NB_BORDER` + items) and
    /// `max(title, longest item + nbSpaceBefore) + 1` total cell columns, with
    /// the outer cells reserved for the corner / bar frame tiles.  Positioned
    /// at `(layout.x, layout.y)` in framebuffer space.
    fn render_menu_box(&self) -> Vec<RenderCommand> {
        let layout = &*MENU_LAYOUT;
        let ts = layout.frame_tileset;
        let step = MENU_FRAME_TILE_SIZE;
        let left = layout.x;
        let top = layout.y;
        // Match Java's `calculateWidthMinimum() + 1` and
        // `NB_BORDER + items.size()` so the frame sits exactly where the small
        // body font lays out the title + items, with the cursor and the four
        // leading spaces accounted for.
        let title_chars = layout.title_text.chars().count();
        let max_item_chars = layout
            .items
            .iter()
            .map(|it| it.text.chars().count() + layout.nb_space_before)
            .max()
            .unwrap_or(0);
        let max_chars = title_chars.max(max_item_chars) as i32;
        let inner_cols = (max_chars - 1).max(1);
        let inner_rows = layout.items.len() as i32 + 3;
        let right = left + (inner_cols + 1) * step;
        let bottom = top + (inner_rows + 1) * step;

        let mut commands = vec![
            blit(ts, layout.left_upper, left, top),
            blit(ts, layout.right_upper, right, top),
            blit(ts, layout.left_lower, left, bottom),
            blit(ts, layout.right_lower, right, bottom),
        ];

        // Top and bottom edge bars.
        for col in 1..=inner_cols {
            let x = left + col * step;
            commands.push(blit(ts, layout.upper_bar, x, top));
            commands.push(blit(ts, layout.lower_bar, x, bottom));
        }

        // Left and right edge bars, and interior fill.
        for row in 1..=inner_rows {
            let y = top + row * step;
            commands.push(blit(ts, layout.left_bar, left, y));
            commands.push(blit(ts, layout.right_bar, right, y));
            for col in 1..=inner_cols {
                commands.push(blit(ts, layout.back_image, left + col * step, y));
            }
        }

        commands
    }

    /// Emits `DrawText` commands for the menu title and item list.
    ///
    /// Mirrors `AbstractStdMenu.drawPicture` + `AbstractMenu.drawCursor` from
    /// the Java reference:
    ///
    /// * Body text for every item is drawn at `(textX + nbSpaceBefore * 6,
    ///   textY + (i + 1) * 8)` in its configured palette color, with the four
    ///   leading spaces baked into the string so the renderer's per-glyph
    ///   `cursor_x` advance lands the text at the same column the original
    ///   used.
    /// * The cursor for the selected row is drawn as a separate, always-white
    ///   glyph at one space-width past the body's left edge, cycling through
    ///   the eight-frame animation at codepoints `0x01..=0x08`.
    fn render_menu_text(&self) -> Vec<RenderCommand> {
        let layout = &*MENU_LAYOUT;
        let base_x = layout.x + layout.text_col_px;
        let base_y = layout.y + layout.text_row_px;
        let line_h = 8_i32; // SHA font renders at 8 px per row.

        let mut commands = vec![RenderCommand::DrawText {
            text: layout.title_text.clone(),
            x: base_x,
            y: base_y,
            color_index: layout.title_color,
            font: FontSize::Small,
        }];

        let spaces = " ".repeat(layout.nb_space_before);
        let cursor_char =
            char::from_u32(u32::from(self.cursor_index % MENU_CURSOR_FRAMES) + 1).unwrap_or(' ');
        for (index, item) in layout.items.iter().enumerate() {
            let y = base_y + line_h + index as i32 * line_h;
            commands.push(RenderCommand::DrawText {
                text: format!("{spaces}{}", item.text),
                x: base_x,
                y,
                color_index: item.color,
                font: FontSize::Small,
            });
            if index == self.selected {
                // Leading space pushes the cursor glyph one column past
                // `base_x`, matching `posCursorX = textX + fontSize` in
                // the Java reference without exposing per-glyph pixel
                // widths to this caller.
                commands.push(RenderCommand::DrawText {
                    text: format!(" {cursor_char}"),
                    x: base_x,
                    y,
                    color_index: MENU_CURSOR_COLOR,
                    font: FontSize::Small,
                });
            }
        }

        commands
    }

    /// Emits commands for the high-score panel rendered in the control area.
    fn render_high_score_panel(&self) -> Vec<RenderCommand> {
        let mut commands = vec![RenderCommand::FillRect {
            x: CONTROL_AREA_X,
            y: CONTROL_AREA_Y,
            width: 64,
            height: 85,
            color: 8,
        }];
        commands.push(RenderCommand::DrawText {
            text: String::from("HI SCORES"),
            x: CONTROL_AREA_X + 5,
            y: CONTROL_AREA_Y + 2,
            color_index: 4,
            font: FontSize::Small,
        });
        // Thin separator line under the HI SCORES header, mirroring the
        // dark red rule in the reference screenshot.
        commands.push(RenderCommand::FillRect {
            x: CONTROL_AREA_X + 2,
            y: CONTROL_AREA_Y + 9,
            width: 60,
            height: 1,
            color: 4,
        });

        // Layout: name column on the left (greenish color 2), score
        // column on the right (orange-ish color 6).  `JILL1.CFG` carries
        // 10 slots; render up to the panel's vertical capacity at a
        // 6-pixel row pitch (the small-font row height).
        const ROW_HEIGHT: i32 = 6;
        const MAX_ROWS: usize = 8;
        for (index, entry) in self.cfg.high_scores().iter().take(MAX_ROWS).enumerate() {
            let y = CONTROL_AREA_Y + 12 + index as i32 * ROW_HEIGHT;
            commands.push(RenderCommand::DrawText {
                text: entry.name().to_string(),
                x: CONTROL_AREA_X + 2,
                y,
                color_index: 2,
                font: FontSize::Small,
            });
            commands.push(RenderCommand::DrawText {
                text: format!("{}", entry.score()),
                x: CONTROL_AREA_X + 40,
                y,
                color_index: 6,
                font: FontSize::Small,
            });
        }

        commands
    }

    /// Emits commands for the instructions/info-box overlay.
    ///
    /// Displays VCL text entry 0 (the game instructions) in a filled overlay
    /// rectangle. The VCL payload embeds `\n` line breaks, which the text
    /// renderer does not interpret; this method splits at `\n` and emits one
    /// [`RenderCommand::DrawText`] per line. Lines beyond [`INFO_BOX_MAX_LINES`]
    /// are dropped so text cannot overflow past the box. Any key press
    /// dismisses this overlay.
    fn render_info_box(&self) -> Vec<RenderCommand> {
        let text = self
            .vcl
            .text_entries()
            .iter()
            .find(|e| e.index() == 0)
            .or_else(|| self.vcl.text_entries().first())
            .map(|e| e.text())
            .unwrap_or("");
        let mut commands = vec![RenderCommand::FillRect {
            x: 92,
            y: 32,
            width: 190,
            height: 130,
            color: 1,
        }];
        for (line_index, line) in text.split('\n').take(INFO_BOX_MAX_LINES).enumerate() {
            commands.push(RenderCommand::DrawText {
                text: line.to_string(),
                x: INFO_BOX_TEXT_X,
                y: INFO_BOX_TEXT_Y + (line_index as i32) * INFO_BOX_LINE_HEIGHT,
                color_index: INFO_BOX_TEXT_COLOR,
                font: FontSize::Small,
            });
        }
        commands
    }

    /// Emits commands for the load-game slot overlay.
    ///
    /// Displays all save slot names from `JILL1.CFG`. Escape dismisses the
    /// overlay; slot selection is deferred to a child issue.
    fn render_load_game(&self) -> Vec<RenderCommand> {
        let mut commands = vec![RenderCommand::FillRect {
            x: 124,
            y: 36,
            width: 132,
            height: 120,
            color: 1,
        }];
        commands.push(RenderCommand::DrawText {
            text: String::from("LOAD GAME"),
            x: 136,
            y: 40,
            color_index: 2,
            font: FontSize::Big,
        });
        for (index, slot) in self.cfg.save_slots().iter().enumerate() {
            let marker = if index == self.load_cursor { ">" } else { " " };
            let name = if slot.name().trim().is_empty() {
                "[EMPTY]"
            } else {
                slot.name()
            };
            commands.push(RenderCommand::DrawText {
                text: format!("{marker} {} {name}", index + 1),
                x: 132,
                y: 56 + index as i32 * 12,
                // Highlight the selected slot (brighter) vs the rest.
                color_index: if index == self.load_cursor { 6 } else { 3 },
                font: FontSize::Small,
            });
        }
        commands
    }
}

/// Tileset entry index that carries the Jill face portrait tiles in
/// `JILL1.SHA`.
///
/// REVERSE-ENGINEERED: design choice from the Java reference's
/// `status_bar_vga.json` `imagesInvenroy` array. Not derivable from SHA
/// structure; future engine config file should expose this.
const PORTRAIT_TILESET: u8 = 24;

/// Per-column x offsets (inventory-area-relative) for the 4 portrait rows.
///
/// REVERSE-ENGINEERED from the Java reference's `status_bar_vga.json`
/// `imagesInvenroy` array. Row 1 column 3 uses `x = 46` instead of `48`
/// to match the original asset's intentional 2-pixel inset for tile 7.
const PORTRAIT_X: [[i32; 4]; 4] = [
    [0, 16, 32, 48],
    [0, 16, 32, 46],
    [0, 16, 32, 48],
    [0, 16, 32, 48],
];

/// Bottom row anchor: bottom edge of row-3 tiles in inventory-area-relative
/// pixels.
///
/// REVERSE-ENGINEERED: with `INVENTORY_AREA_Y = 107`, anchoring the row-3
/// bottom edge at framebuffer y = 175 (immediately above the lower
/// status-bar frame bar at y = 176) yields `dy_bottom = 175 - 107 + 1 = 69`.
/// Tile 15 has SHA height 20 and tiles 12-14 have height 22; per-tile
/// `dy = PORTRAIT_ROW3_BOTTOM - h` keeps every bottom edge aligned.
const PORTRAIT_ROW3_BOTTOM: i32 = 69;

/// Computes the 16-entry portrait tile layout `(dx, dy, tile_index)` from
/// the SHA tileset heights.
///
/// Rows 0-2 stack vertically using the maximum tile height in each row
/// (uniform 16 for tileset 24). Row 3 anchors each tile individually so
/// its bottom edge lines up with [`PORTRAIT_ROW3_BOTTOM`], accommodating
/// the varying heights of tiles 12-15 (22 vs 20 px).
///
/// Falls back to a flat 16-pixel row height when the SHA tileset is
/// absent (e.g. synthetic test fixtures); positions are still emitted so
/// downstream tests that count render commands continue to pass.
fn compute_portrait_tiles(sha: &ShaFile) -> [(i32, i32, u16); 16] {
    let tileset = sha
        .tilesets()
        .iter()
        .find(|ts| ts.entry_index() == usize::from(PORTRAIT_TILESET));
    let tile_h = |idx: u16| -> i32 {
        tileset
            .and_then(|ts| ts.tiles().get(usize::from(idx)))
            .map(|t| i32::from(t.height()))
            .unwrap_or(BLOCK_SIZE_I)
    };

    let mut result = [(0i32, 0i32, 0u16); 16];
    let mut dy = 0i32;
    for row in 0..3usize {
        let mut row_h = 0i32;
        for col in 0..4usize {
            let tile = (row * 4 + col) as u16;
            result[row * 4 + col] = (PORTRAIT_X[row][col], dy, tile);
            row_h = row_h.max(tile_h(tile));
        }
        dy += row_h;
    }
    for col in 0..4usize {
        let tile = (12 + col) as u16;
        let row3_dy = PORTRAIT_ROW3_BOTTOM - tile_h(tile);
        result[12 + col] = (PORTRAIT_X[3][col], row3_dy, tile);
    }
    result
}

/// Emits the 16 portrait blits for the inventory area.
///
/// Translates the inventory-area-relative offsets in `portrait_tiles` into
/// framebuffer coordinates via [`INVENTORY_AREA_X`] / [`INVENTORY_AREA_Y`].
fn render_jill_portrait(portrait_tiles: &[(i32, i32, u16); 16]) -> Vec<RenderCommand> {
    portrait_tiles
        .iter()
        .map(|(dx, dy, tile)| RenderCommand::Blit {
            tileset: PORTRAIT_TILESET,
            tile: *tile,
            x: INVENTORY_AREA_X + dx,
            y: INVENTORY_AREA_Y + dy,
            opaque: false,
            clip: None,
        })
        .collect()
}

/// Builds a framebuffer-absolute `Blit` command for the menu frame.
///
/// `start_menu.json` `x`/`y` are screen-absolute in the Java reference port:
/// `ClassicMenu` builds its picture into an off-screen buffer, and
/// `AbstractMenuJillLevel.paint()` draws that buffer with
/// `g.drawImage(menuPicture, menu.getX(), menu.getY())` directly into the
/// 320x200 framebuffer.  Do not offset by the game-area origin here.
fn blit(tileset: u8, tile: u16, x: i32, y: i32) -> RenderCommand {
    RenderCommand::Blit {
        tileset,
        tile,
        x,
        y,
        opaque: false,
        clip: None,
    }
}

/// Parses `start_menu.json` into a [`MenuLayout`].
///
/// # Panics
///
/// Panics if the embedded JSON is structurally invalid, because this is an
/// unrecoverable programming error (the JSON is compile-time embedded).
fn parse_menu_layout() -> MenuLayout {
    let value: serde_json::Value =
        serde_json::from_str(START_MENU_JSON).expect("embedded start_menu.json must be valid JSON");

    let get_i = |obj: &serde_json::Value, key: &str, default: i64| -> i32 {
        obj.get(key).and_then(|v| v.as_i64()).unwrap_or(default) as i32
    };
    let get_u = |obj: &serde_json::Value, key: &str, default: u64| -> u8 {
        obj.get(key).and_then(|v| v.as_u64()).unwrap_or(default) as u8
    };
    let get_str = |obj: &serde_json::Value, key: &str, default: &str| -> String {
        obj.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(default)
            .to_string()
    };
    let tile_ref = |key: &str| -> (u8, u16) {
        let obj = value.get(key);
        let ts = obj
            .and_then(|o| o.get("tileset"))
            .and_then(|v| v.as_u64())
            .unwrap_or(7) as u8;
        let tile = obj
            .and_then(|o| o.get("tile"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as u16;
        (ts, tile)
    };

    let x = get_i(&value, "x", 72);
    let y = get_i(&value, "y", 64);
    // textX and textY in the JSON are pixel offsets inside the menu box.
    let text_col_px = get_i(&value, "textX", 0);
    let text_row_px = get_i(&value, "textY", 0);
    let nb_space_before = get_i(&value, "nbSpaceBefore", 0).max(0) as usize;

    let title_obj = value
        .get("title")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let title_color = get_u(&title_obj, "color", 7);
    let title_text = get_str(&title_obj, "text", "pick a choice :");

    let items = value
        .get("item")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|entry| MenuItem {
                    text: get_str(entry, "text", ""),
                    color: get_u(entry, "color", 7),
                    value: get_i(entry, "value", 0),
                })
                .collect()
        })
        .unwrap_or_default();

    // The shipped `start_menu.json` labels the left and right corner /
    // side-bar tiles with the wrong handedness compared to how the SHA
    // pixels actually render: `leftUpperCorner` carries the tile that
    // visually paints the top-RIGHT corner, `leftBar` carries the
    // right-side vertical bar, and so on.  Verified by inspecting the
    // pixel highlights on tileset 7 tiles 1-9 in the indexed atlas:
    // tile 1 has its lit edges on the top + left (top-left corner),
    // tile 3 on the top + right (top-right corner), tile 4's lit edge
    // is the left column (left vertical bar), tile 5's the right
    // column.  Pre-swap the assignments so the renderer can keep using
    // semantic field names.
    let (frame_tileset, left_upper) = tile_ref("rightUpperCorner");
    let (_, right_upper) = tile_ref("leftUpperCorner");
    let (_, left_lower) = tile_ref("rightLowerCorner");
    let (_, right_lower) = tile_ref("leftLowerCorner");
    let (_, upper_bar) = tile_ref("upperBar");
    let (_, lower_bar) = tile_ref("lowerBar");
    let (_, left_bar) = tile_ref("rightBar");
    let (_, right_bar) = tile_ref("leftBar");
    let back_image = value
        .get("backImage")
        .and_then(|o| o.get("tile"))
        .and_then(|v| v.as_u64())
        .unwrap_or(9) as u16;

    MenuLayout {
        x,
        y,
        text_col_px,
        text_row_px,
        nb_space_before,
        title_color,
        title_text,
        items,
        frame_tileset,
        right_upper,
        left_upper,
        right_lower,
        left_lower,
        upper_bar,
        lower_bar,
        right_bar,
        left_bar,
        back_image,
    }
}

#[cfg(test)]
mod tests {
    use super::StartMenuScreen;
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{
        ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition,
    };
    use openjill_data::cfg::CfgFile;
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;
    use openjill_data::sha::ShaFile;
    use openjill_data::vcl::VclFile;

    /// Minimal byte counts for synthetic fixture construction.
    const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;
    const VCL_MIN_BYTES: usize = 400 + 40 * 4 + 40 * 2;
    const CFG_MIN_BYTES: usize = 10 * 10 + 20 + 10 * 4 + 6 * 12 + 2 + 2 + 6 * 2 + 2 + 2 + 2;
    /// SHA header-only byte count (128 header entries × 4 bytes + 128 × 2-byte
    /// header tail) matching [`crate::asset_cache::AssetCache::synthetic`].
    const SHA_HEADER_BYTES: usize = 128 * 4 + 128 * 2;

    /// Builds a minimal all-zero `JnFile` for tests.
    fn zero_jn() -> JnFile {
        JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse")
    }

    /// Builds a minimal empty `DmaFile` for tests.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Builds a minimal all-zero `ShaFile` (header only, zero tilesets) for
    /// tests that only need the start-menu screen to construct.
    fn zero_sha() -> ShaFile {
        ShaFile::from_bytes(vec![0u8; SHA_HEADER_BYTES]).expect("zero SHA should parse")
    }

    /// Builds a minimal all-zero `VclFile` for tests.
    fn zero_vcl() -> VclFile {
        VclFile::from_bytes(vec![0u8; VCL_MIN_BYTES]).expect("zero VCL should parse")
    }

    /// Builds a `VclFile` whose text entry 0 contains the supplied payload.
    ///
    /// The synthetic fixture writes the bytes at offset 700 (past the
    /// offset/length tables) and seeds slot 0's offset/length table entry to
    /// point at them, so `text_entries()` exposes one entry with `index() == 0`
    /// and the requested text.
    fn vcl_with_entry_zero(payload: &str) -> VclFile {
        let bytes = payload.as_bytes();
        let length = u16::try_from(bytes.len())
            .expect("VCL text fixture payload must fit in u16 (declared_length field is u16le)");
        let text_offset: usize = 700;
        let mut buf = vec![0u8; text_offset + bytes.len()];

        let offset_pos = 400_usize;
        buf[offset_pos..offset_pos + 4].copy_from_slice(&(text_offset as u32).to_le_bytes());

        let length_pos = 400_usize + 40 * 4;
        buf[length_pos..length_pos + 2].copy_from_slice(&length.to_le_bytes());

        buf[text_offset..text_offset + bytes.len()].copy_from_slice(bytes);

        VclFile::from_bytes(buf).expect("VCL fixture should parse")
    }

    /// Builds a minimal all-zero `CfgFile` for tests.
    fn zero_cfg() -> CfgFile {
        CfgFile::from_bytes(vec![0u8; CFG_MIN_BYTES], "JN1").expect("zero CFG should parse")
    }

    /// Creates a `StartMenuScreen` with all-zero synthetic fixtures.
    fn menu() -> StartMenuScreen {
        StartMenuScreen::new(zero_jn(), empty_dma(), zero_vcl(), zero_cfg(), &zero_sha())
    }

    /// Presses one input for a single tick, then releases it with an empty
    /// tick. Menu input is latched to the rising edge (one key press performs
    /// exactly one action), so every distinct action in a test must be
    /// followed by a release before the next press registers. Returns the
    /// transition produced by the press tick.
    fn press(screen: &mut StartMenuScreen, cmd: InputCommand) -> Option<ScreenTransition> {
        let mut input = ActiveInput::new();
        input.insert(cmd);
        let transition = screen.tick(&input, &mut RuntimeState::new()).transition;
        screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        transition
    }

    /// Unit under test: `StartMenuScreen::tick` — confirms item 0 ("play") via
    /// ThrowItem (Ctrl) after the default selection is already at index 0.
    ///
    /// Preconditions: fresh screen with `selected = 0` (default).
    ///
    /// Invariants asserted: ThrowItem confirm on item value 0 returns
    /// `ScreenTransition::Map`.
    #[test]
    fn confirm_play_item_transitions_to_map() {
        let mut screen = menu();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::ThrowItem);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::Map));
    }

    /// Unit under test: the `Ctrl+P` intro-play cheat on the base menu.
    ///
    /// Preconditions: no overlay; `InputCommand::PlayIntro` (the chord) pressed
    /// this tick, together with the `ThrowItem` that `Ctrl` also produces.
    ///
    /// Invariants asserted: it transitions straight to the playable intro level
    /// (`INTRO.JN1`), taking priority over the `Ctrl`-as-`ThrowItem` confirm.
    #[test]
    fn ctrl_p_plays_the_intro_level() {
        let mut screen = menu();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::PlayIntro);
        input.insert(InputCommand::ThrowItem);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(
            result.transition,
            Some(ScreenTransition::Level {
                file: String::from("INTRO.JN1"),
                number: 0,
            })
        );
    }

    /// Unit under test: Escape (`InputCommand::Pause`) quits from the start menu.
    ///
    /// Preconditions: no overlay active.
    ///
    /// Invariants asserted: `ScreenTransition::Quit` is returned.
    #[test]
    fn escape_quits_from_base_menu() {
        let mut screen = menu();
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Pause);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::Quit));
    }

    /// Unit under test: the per-command rising-edge latch acts on keys newly
    /// pressed this tick even while an unrelated key is still held.
    ///
    /// Preconditions: ArrowDown held for one tick, then Escape pressed while
    /// ArrowDown is still held.
    ///
    /// Invariants asserted: the still-held ArrowDown does not repeat (no extra
    /// navigation), and the newly pressed Escape registers as `Quit`.
    #[test]
    fn held_key_does_not_block_an_independent_key() {
        let mut screen = menu();

        // Tick 1: ArrowDown held alone.
        let mut down = ActiveInput::new();
        down.insert(InputCommand::Duck);
        assert_eq!(
            screen.tick(&down, &mut RuntimeState::new()).transition,
            None
        );

        // Tick 2: Escape pressed while ArrowDown is still held. ArrowDown is no
        // longer a rising edge (held last tick), but Escape is new and quits.
        let mut down_and_esc = ActiveInput::new();
        down_and_esc.insert(InputCommand::Duck);
        down_and_esc.insert(InputCommand::Pause);
        assert_eq!(
            screen
                .tick(&down_and_esc, &mut RuntimeState::new())
                .transition,
            Some(ScreenTransition::Quit)
        );
    }

    /// Unit under test: Escape (`InputCommand::Pause`) with an active load-game
    /// overlay dismisses the overlay rather than quitting.
    ///
    /// Preconditions: navigate to item 1 ("restore") and confirm it to open the
    /// load-game overlay.
    ///
    /// Invariants asserted: first Escape dismisses the overlay (no transition);
    /// second Escape quits.
    #[test]
    fn escape_dismisses_load_game_overlay_before_quit() {
        let mut screen = menu();
        open_load_overlay(&mut screen);

        // First Escape dismisses the overlay without quitting; a held Escape is
        // latched, so it does not cascade into a quit on the same press.
        assert_eq!(
            press(&mut screen, InputCommand::Pause),
            None,
            "first escape must dismiss overlay, not quit"
        );

        // A second, distinct Escape press quits.
        assert_eq!(
            press(&mut screen, InputCommand::Pause),
            Some(ScreenTransition::Quit)
        );
    }

    /// Opens the load-game overlay (navigate to item 1, confirm) and releases
    /// the keys so the overlay debounce is reset for the next press.
    fn open_load_overlay(screen: &mut StartMenuScreen) {
        press(screen, InputCommand::Duck); // navigate to item 1 ("restore")
        assert_eq!(
            press(screen, InputCommand::ThrowItem),
            None,
            "opening the load overlay must not transition"
        );
    }

    /// Unit under test: confirming in the load-game overlay emits a
    /// [`ScreenTransition::PerformLoad`] for the highlighted slot.
    #[test]
    fn load_overlay_confirm_emits_perform_load() {
        let mut screen = menu();
        open_load_overlay(&mut screen);

        assert_eq!(
            press(&mut screen, InputCommand::ThrowItem),
            Some(ScreenTransition::PerformLoad { slot: 0 })
        );
    }

    /// Unit under test: down moves the load-overlay cursor before confirm.
    #[test]
    fn load_overlay_down_then_confirm_loads_that_slot() {
        let mut screen = menu();
        open_load_overlay(&mut screen);

        press(&mut screen, InputCommand::Duck); // cursor 0 -> 1

        assert_eq!(
            press(&mut screen, InputCommand::ThrowItem),
            Some(ScreenTransition::PerformLoad { slot: 1 })
        );
    }

    /// Unit under test: ArrowUp (`InputCommand::Up`) moves the load-overlay
    /// cursor up, wrapping from the first slot to the last.
    #[test]
    fn load_overlay_arrow_up_wraps_to_last_slot() {
        let mut screen = menu();
        open_load_overlay(&mut screen);

        press(&mut screen, InputCommand::Up); // cursor 0 -> last (wrap)

        // zero_cfg carries six save slots, so wrapping up from 0 lands on 5.
        assert_eq!(
            press(&mut screen, InputCommand::ThrowItem),
            Some(ScreenTransition::PerformLoad { slot: 5 })
        );
    }

    /// Unit under test: `StartMenuScreen::tick` — `InputCommand::Duck` navigates
    /// down, and confirming a Story item returns `ScreenTransition::Story`.
    ///
    /// Preconditions: navigate from item 0 to item 2 ("story") using two Duck
    /// inputs, then confirm.
    ///
    /// Invariants asserted: `ScreenTransition::Story` is returned.
    #[test]
    fn navigate_to_story_and_confirm() {
        let mut screen = menu();
        // Skip item 0 ("play") and item 1 ("restore").
        press(&mut screen, InputCommand::Duck);
        press(&mut screen, InputCommand::Duck);

        assert_eq!(
            press(&mut screen, InputCommand::ThrowItem),
            Some(ScreenTransition::Story)
        );
    }

    /// Unit under test: navigating past the last item wraps to item 0.
    ///
    /// Preconditions: `selected = last item` before pressing Duck.
    ///
    /// Invariants asserted: after the wrap, confirming item 0 returns
    /// `ScreenTransition::Map`.
    #[test]
    fn selection_wraps_from_last_to_first() {
        let mut screen = menu();
        let item_count = super::MENU_LAYOUT.items.len();
        // Navigate past the end to wrap back to item 0 ("play").
        for _ in 0..item_count {
            press(&mut screen, InputCommand::Duck);
        }
        assert_eq!(
            press(&mut screen, InputCommand::ThrowItem),
            Some(ScreenTransition::Map)
        );
    }

    /// Unit under test: the first tick with no input produces at least one
    /// `DrawText` command for the menu title.
    ///
    /// Preconditions: fresh screen, empty input.
    ///
    /// Invariants asserted: at least one `DrawText` command is present in the
    /// result.
    #[test]
    fn idle_tick_emits_draw_text_commands() {
        let mut screen = menu();
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(
            result
                .commands
                .iter()
                .any(|c| matches!(c, RenderCommand::DrawText { .. })),
            "idle tick should emit at least one DrawText command"
        );
    }

    /// Opens the info-box overlay on `screen` by selecting the "instructions"
    /// item (start_menu.json item 3) and confirming it. Returns the tick
    /// commands emitted by the confirm tick (which already include the overlay).
    fn open_info_box_overlay(screen: &mut StartMenuScreen) -> Vec<RenderCommand> {
        // Navigate to item 3 ("instructions").
        for _ in 0..3 {
            press(screen, InputCommand::Duck);
        }
        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let commands = screen.tick(&confirm, &mut RuntimeState::new()).commands;
        screen.tick(&ActiveInput::new(), &mut RuntimeState::new()); // release
        commands
    }

    /// Collects info-box `DrawText` commands from a render command list.
    ///
    /// Filters by the info-box `x` column and `color_index` so unrelated
    /// `DrawText` commands (menu title, items, high-score panel) are excluded.
    fn info_box_lines(commands: &[RenderCommand]) -> Vec<(i32, String)> {
        commands
            .iter()
            .filter_map(|c| match c {
                RenderCommand::DrawText {
                    text,
                    x,
                    y,
                    color_index,
                    ..
                } if *x == super::INFO_BOX_TEXT_X && *color_index == super::INFO_BOX_TEXT_COLOR => {
                    Some((*y, text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Unit under test: `render_info_box` splits VCL entry 0 on `\n` and emits
    /// one `DrawText` per line at stepped y-coordinates.
    ///
    /// Preconditions: VCL entry 0 contains three lines separated by `\n`;
    /// instructions overlay is opened by selecting item 3 and confirming.
    ///
    /// Invariants asserted: three info-box `DrawText` commands are emitted, in
    /// source order, with `y` advancing by `INFO_BOX_LINE_HEIGHT` per line and
    /// the first line at `INFO_BOX_TEXT_Y`.
    #[test]
    fn info_box_splits_text_on_newlines() {
        let vcl = vcl_with_entry_zero("LINE 1\nLINE 2\nLINE 3");
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg(), &zero_sha());

        let commands = open_info_box_overlay(&mut screen);
        let lines = info_box_lines(&commands);

        assert_eq!(lines.len(), 3, "expected one DrawText per VCL line");
        assert_eq!(lines[0], (super::INFO_BOX_TEXT_Y, String::from("LINE 1")));
        assert_eq!(
            lines[1],
            (
                super::INFO_BOX_TEXT_Y + super::INFO_BOX_LINE_HEIGHT,
                String::from("LINE 2"),
            )
        );
        assert_eq!(
            lines[2],
            (
                super::INFO_BOX_TEXT_Y + 2 * super::INFO_BOX_LINE_HEIGHT,
                String::from("LINE 3"),
            )
        );
    }

    /// Unit under test: `render_info_box` caps emitted lines at
    /// `INFO_BOX_MAX_LINES` to avoid overflowing the box.
    ///
    /// Preconditions: VCL entry 0 contains 20 newline-separated lines.
    ///
    /// Invariants asserted: only `INFO_BOX_MAX_LINES` info-box `DrawText`
    /// commands are emitted, and they correspond to the first
    /// `INFO_BOX_MAX_LINES` lines of the source text.
    #[test]
    fn info_box_clips_lines_beyond_max() {
        let lines_in: Vec<String> = (0..20).map(|i| format!("L{i}")).collect();
        let vcl = vcl_with_entry_zero(&lines_in.join("\n"));
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg(), &zero_sha());

        let commands = open_info_box_overlay(&mut screen);
        let lines_out = info_box_lines(&commands);

        assert_eq!(lines_out.len(), super::INFO_BOX_MAX_LINES);
        for (i, (y, text)) in lines_out.iter().enumerate() {
            assert_eq!(
                *y,
                super::INFO_BOX_TEXT_Y + (i as i32) * super::INFO_BOX_LINE_HEIGHT
            );
            assert_eq!(text, &lines_in[i]);
        }
    }

    /// Unit under test: `render_info_box` emits a single line when VCL entry 0
    /// has no embedded newlines, leaving existing single-line `DrawText`
    /// behavior unchanged.
    ///
    /// Preconditions: VCL entry 0 contains a single line with no `\n`.
    ///
    /// Invariants asserted: exactly one info-box `DrawText` command is emitted
    /// at `INFO_BOX_TEXT_Y` with the full text payload.
    #[test]
    fn info_box_single_line_unchanged() {
        let vcl = vcl_with_entry_zero("ONLY LINE");
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg(), &zero_sha());

        let commands = open_info_box_overlay(&mut screen);
        let lines = info_box_lines(&commands);

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            (super::INFO_BOX_TEXT_Y, String::from("ONLY LINE"))
        );
    }
}
