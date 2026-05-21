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
use openjill_core::layout::{
    BLOCK_SIZE_I, CONTROL_AREA_X, CONTROL_AREA_Y, GAME_AREA_X, GAME_AREA_Y,
};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, InputCommand, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
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
    /// Menu box top-left X in game-area pixel coordinates.
    x: i32,
    /// Menu box top-left Y in game-area pixel coordinates.
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
        match self.overlay {
            Overlay::InfoBox => commands.extend(self.render_info_box()),
            Overlay::LoadGame => commands.extend(self.render_load_game()),
            Overlay::None => {}
        }
        commands
    }

    /// Emits `Blit` commands for the tileset-7 menu box frame.
    ///
    /// The box occupies `(items.len() + 2)` tile rows (title + items + padding)
    /// and 10 tile columns, positioned at `(layout.x, layout.y)` in game-area
    /// space.
    fn render_menu_box(&self) -> Vec<RenderCommand> {
        let layout = &*MENU_LAYOUT;
        let ts = layout.frame_tileset;
        let left = layout.x;
        let top = layout.y;
        // Width: 9 inner fill columns + 2 border columns = 11 tiles total.
        let inner_cols = 9_i32;
        // Height: 1 title row + items count + 1 padding row.
        let inner_rows = layout.items.len() as i32 + 2;
        let right = left + (inner_cols + 1) * BLOCK_SIZE_I;
        let bottom = top + inner_rows * BLOCK_SIZE_I;

        let mut commands = vec![
            blit(ts, layout.left_upper, left, top),
            blit(ts, layout.right_upper, right, top),
            blit(ts, layout.left_lower, left, bottom),
            blit(ts, layout.right_lower, right, bottom),
        ];

        // Top and bottom edge bars.
        for col in 1..=inner_cols {
            let x = left + col * BLOCK_SIZE_I;
            commands.push(blit(ts, layout.upper_bar, x, top));
            commands.push(blit(ts, layout.lower_bar, x, bottom));
        }

        // Left and right edge bars, and interior fill.
        for row in 1..inner_rows {
            let y = top + row * BLOCK_SIZE_I;
            commands.push(blit(ts, layout.left_bar, left, y));
            commands.push(blit(ts, layout.right_bar, right, y));
            for col in 1..=inner_cols {
                commands.push(blit(ts, layout.back_image, left + col * BLOCK_SIZE_I, y));
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
            x: GAME_AREA_X + base_x,
            y: GAME_AREA_Y + base_y,
            color_index: layout.title_color,
        }];

        let spaces = " ".repeat(layout.nb_space_before);
        for (index, item) in layout.items.iter().enumerate() {
            let y = base_y + line_h + index as i32 * line_h;
            let prefix = if index == self.selected { ">" } else { " " };
            let text = format!("{prefix}{spaces}{}", item.text);
            commands.push(RenderCommand::DrawText {
                text,
                x: GAME_AREA_X + base_x,
                y: GAME_AREA_Y + y,
                color_index: item.color,
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
        });

        for (index, entry) in self.cfg.high_scores().iter().take(5).enumerate() {
            let y = CONTROL_AREA_Y + 16 + index as i32 * 12;
            commands.push(RenderCommand::DrawText {
                text: format!("{:>7}", entry.score()),
                x: CONTROL_AREA_X + 2,
                y,
                color_index: 6,
            });
            commands.push(RenderCommand::DrawText {
                text: entry.name().to_string(),
                x: CONTROL_AREA_X + 30,
                y,
                color_index: 2,
            });
        }

        commands
    }

    /// Emits commands for the instructions/info-box overlay.
    ///
    /// Displays VCL text entry 0 (the game instructions) in a filled overlay
    /// rectangle. Any key press dismisses this overlay.
    fn render_info_box(&self) -> Vec<RenderCommand> {
        let text = self
            .vcl
            .text_entries()
            .iter()
            .find(|e| e.index() == 0)
            .or_else(|| self.vcl.text_entries().first())
            .map(|e| e.text().to_string())
            .unwrap_or_default();
        vec![
            RenderCommand::FillRect {
                x: 92,
                y: 32,
                width: 190,
                height: 130,
                color: 1,
            },
            RenderCommand::DrawText {
                text,
                x: 100,
                y: 44,
                color_index: 7,
            },
        ]
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
        });
        for (index, slot) in self.cfg.save_slots().iter().enumerate() {
            commands.push(RenderCommand::DrawText {
                text: format!("{} {}", index + 1, slot.name()),
                x: 132,
                y: 56 + index as i32 * 12,
                color_index: 3,
            });
        }
        commands
    }
}

/// Builds a game-area-relative `Blit` command.
fn blit(tileset: u8, tile: u16, game_x: i32, game_y: i32) -> RenderCommand {
    RenderCommand::Blit {
        tileset,
        tile,
        x: GAME_AREA_X + game_x,
        y: GAME_AREA_Y + game_y,
        opaque: false,
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

    let (frame_tileset, right_upper) = tile_ref("rightUpperCorner");
    let (_, left_upper) = tile_ref("leftUpperCorner");
    let (_, right_lower) = tile_ref("rightLowerCorner");
    let (_, left_lower) = tile_ref("leftLowerCorner");
    let (_, upper_bar) = tile_ref("upperBar");
    let (_, lower_bar) = tile_ref("lowerBar");
    let (_, right_bar) = tile_ref("rightBar");
    let (_, left_bar) = tile_ref("leftBar");
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
}
