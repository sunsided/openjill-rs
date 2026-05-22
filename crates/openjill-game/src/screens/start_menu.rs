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
use openjill_core::layout::{CONTROL_AREA_X, CONTROL_AREA_Y, INVENTORY_AREA_X, INVENTORY_AREA_Y};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, FontSize, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::cfg::CfgFile;
use openjill_data::dma::DmaFile;
use openjill_data::jn::JnFile;
use openjill_data::vcl::VclFile;
use std::sync::LazyLock;

/// Embedded `start_menu.json` layout resource from the Java reference port.
const START_MENU_JSON: &str =
    include_str!("../../../../OpenJill/src/main/resources/start_menu.json");

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

/// Cursor glyph drawn before the currently selected menu item.
///
/// The original DOS Jill draws a small bullet/circle here; ASCII codepoint
/// `0x07` (bell) maps to the bullet glyph in the SHA font tilesets, which
/// is the closest match to the reference cursor.
const MENU_CURSOR_CHAR: char = '\u{0007}';

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
}

impl StartMenuScreen {
    /// Creates the start menu screen from pre-loaded episode data.
    pub fn new(intro: JnFile, dma: DmaFile, vcl: VclFile, cfg: CfgFile) -> Self {
        Self {
            intro,
            dma,
            vcl,
            cfg,
            selected: 0,
            overlay: Overlay::None,
        }
    }
}

impl ScreenHandler for StartMenuScreen {
    /// Renders one start-menu tick, processes input, and optionally transitions.
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        let transition = self.process_input(input);
        let commands = self.render_frame();
        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }
}

impl StartMenuScreen {
    /// Processes the active input set and updates overlay/selection/transition.
    fn process_input(&mut self, input: &ActiveInput) -> Option<ScreenTransition> {
        // Any key dismisses the info-box overlay without further action,
        // including Escape — checked before the Quit path so Escape does not
        // exit the game while the overlay is visible.
        if self.overlay == Overlay::InfoBox && !input.is_empty() {
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

        // Load-game overlay is modal: no navigation or confirm while it is open.
        if self.overlay == Overlay::LoadGame {
            return None;
        }

        let layout = &*MENU_LAYOUT;
        if layout.items.is_empty() {
            return None;
        }

        // Navigate down: Duck (ArrowDown) or NextInventory (Tab).
        if input.contains(&InputCommand::Duck) || input.contains(&InputCommand::NextInventory) {
            self.selected = (self.selected + 1) % layout.items.len();
            return None;
        }

        // Navigate up: PrevInventory (Backspace).
        if input.contains(&InputCommand::PrevInventory) {
            let len = layout.items.len();
            self.selected = (self.selected + len - 1) % len;
            return None;
        }

        // Confirm selection: ThrowItem (Ctrl) or Jump (Space / Alt / ArrowUp).
        if input.contains(&InputCommand::ThrowItem) || input.contains(&InputCommand::Jump) {
            let value = layout.items[self.selected].value;
            return self.apply_value(value);
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
        commands.extend(self.render_menu_box());
        commands.extend(self.render_menu_text());
        commands.extend(self.render_high_score_panel());
        commands.extend(render_jill_portrait());
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
    /// The selected item is highlighted by drawing an arrow (`">"`) before it.
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
        for (index, item) in layout.items.iter().enumerate() {
            let y = base_y + line_h + index as i32 * line_h;
            let prefix = if index == self.selected {
                MENU_CURSOR_CHAR
            } else {
                ' '
            };
            let text = format!("{prefix}{spaces}{}", item.text);
            commands.push(RenderCommand::DrawText {
                text,
                x: base_x,
                y,
                color_index: item.color,
                font: FontSize::Small,
            });
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
            commands.push(RenderCommand::DrawText {
                text: format!("{} {}", index + 1, slot.name()),
                x: 132,
                y: 56 + index as i32 * 12,
                color_index: 3,
                font: FontSize::Small,
            });
        }
        commands
    }
}

/// Tileset entry index that carries the Jill face portrait tiles in
/// `JILL1.SHA`.
const PORTRAIT_TILESET: u8 = 24;

/// Tile placement of the Jill face portrait inside the inventory area.
///
/// Mirrors the `imagesInvenroy` array in `status_bar_vga.json`: 16 tiles
/// arranged as a 4x4 grid covering 64 pixels horizontally by ~68 pixels
/// vertically.  The third entry in each tuple is the source tile index
/// inside [`PORTRAIT_TILESET`].
///
/// The shipped JSON places every row 3 entry at `y = 48`, which only
/// works when all bottom-row tiles share the same height.  Tileset 24
/// row 3 actually carries three 22-pixel tall tiles (12, 13, 14) and one
/// 20-pixel tall tile (15), so a uniform `dy = 48` leaves tiles 12/13/14
/// bleeding one pixel into the lower status-bar frame and tile 15 one
/// pixel short of the inventory area's bottom edge.  This table uses
/// per-tile `dy` values (47 for the 22-tall tiles, 49 for the 20-tall
/// tile 15) so every bottom-row tile bottom lines up at framebuffer
/// y = 175, immediately above the lower frame bar at y = 176.
const PORTRAIT_TILES: [(i32, i32, u16); 16] = [
    (0, 0, 0),
    (16, 0, 1),
    (32, 0, 2),
    (48, 0, 3),
    (0, 16, 4),
    (16, 16, 5),
    (32, 16, 6),
    (46, 16, 7),
    (0, 32, 8),
    (16, 32, 9),
    (32, 32, 10),
    (48, 32, 11),
    (0, 47, 12),
    (16, 47, 13),
    (32, 47, 14),
    (48, 49, 15),
];

/// Emits the 16 portrait blits for the inventory area.
///
/// The status-bar JSON places the tiles at inventory-area-relative
/// positions; this helper translates each into framebuffer coordinates
/// via [`INVENTORY_AREA_X`] / [`INVENTORY_AREA_Y`].
fn render_jill_portrait() -> Vec<RenderCommand> {
    PORTRAIT_TILES
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
    use openjill_data::vcl::VclFile;

    /// Minimal byte counts for synthetic fixture construction.
    const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;
    const VCL_MIN_BYTES: usize = 400 + 40 * 4 + 40 * 2;
    const CFG_MIN_BYTES: usize = 10 * 10 + 20 + 10 * 4 + 6 * 12 + 2 + 2 + 6 * 2 + 2 + 2 + 2;

    /// Builds a minimal all-zero `JnFile` for tests.
    fn zero_jn() -> JnFile {
        JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse")
    }

    /// Builds a minimal empty `DmaFile` for tests.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
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
        StartMenuScreen::new(zero_jn(), empty_dma(), zero_vcl(), zero_cfg())
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
        // Navigate to item index 1 ("restore").
        let mut down = ActiveInput::new();
        down.insert(InputCommand::Duck);
        screen.tick(&down, &mut RuntimeState::new());

        // Confirm to open load-game overlay.
        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let open = screen.tick(&confirm, &mut RuntimeState::new());
        assert_eq!(
            open.transition, None,
            "opening load-game overlay must not transition"
        );

        // First Escape dismisses the overlay.
        let mut esc = ActiveInput::new();
        esc.insert(InputCommand::Pause);
        let dismiss = screen.tick(&esc, &mut RuntimeState::new());
        assert_eq!(
            dismiss.transition, None,
            "first escape must dismiss overlay"
        );

        // Second Escape quits.
        let quit = screen.tick(&esc, &mut RuntimeState::new());
        assert_eq!(quit.transition, Some(ScreenTransition::Quit));
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
        let mut down = ActiveInput::new();
        down.insert(InputCommand::Duck);
        // Skip item 0 ("play") and item 1 ("restore").
        screen.tick(&down, &mut RuntimeState::new());
        screen.tick(&down, &mut RuntimeState::new());

        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let result = screen.tick(&confirm, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::Story));
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
        let mut down = ActiveInput::new();
        down.insert(InputCommand::Duck);
        // Navigate past the end to wrap.
        for _ in 0..item_count {
            screen.tick(&down, &mut RuntimeState::new());
        }
        // Now at item 0 ("play").
        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        let result = screen.tick(&confirm, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::Map));
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
        let mut down = ActiveInput::new();
        down.insert(InputCommand::Duck);
        for _ in 0..3 {
            screen.tick(&down, &mut RuntimeState::new());
        }
        let mut confirm = ActiveInput::new();
        confirm.insert(InputCommand::ThrowItem);
        screen.tick(&confirm, &mut RuntimeState::new()).commands
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
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg());

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
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg());

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
        let mut screen = StartMenuScreen::new(zero_jn(), empty_dma(), vcl, zero_cfg());

        let commands = open_info_box_overlay(&mut screen);
        let lines = info_box_lines(&commands);

        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            (super::INFO_BOX_TEXT_Y, String::from("ONLY LINE"))
        );
    }
}
