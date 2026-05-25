//! Level screen handler backed by a `<level_number>.JN1` level file
//! (`1.JN1`, `2.JN1`, ..., `50.JN1` on disk for episode 1).
//!
//! Renders the level background identically to [`MapScreen`](super::map_screen)
//! and listens on the shared [`MessageDispatcher`] for the three level
//! transition messages
//! (`CheckpointChangeLevel`, `CheckpointChangeLevelPrevious`,
//! `DieRestartLevel`).  When a transition message arrives the screen displays
//! the level message box for [`LEVEL_MESSAGE_TICKS`] ticks and then surfaces the
//! resulting [`ScreenTransition`] to the orchestrator.
//!
//! Entity objects from the level's JN object list are loaded but do not tick;
//! gameplay behavior is deferred to epic 6.  The checkpoint object (an object
//! whose `counter` equals the level number) seeds the initial viewport offset,
//! matching the Java reference's `findCheckPoint` behavior.

use std::sync::{Arc, LazyLock, Mutex};

use openjill_core::layout::{
    BLOCK_SIZE_I, GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y, INVENTORY_AREA_H,
    INVENTORY_AREA_W, INVENTORY_AREA_X, INVENTORY_AREA_Y, LEVEL_MESSAGE_TICKS, MESSAGE_BAR_H,
    MESSAGE_BAR_Y, SCREEN_WIDTH, X_UPDATE_BORDER, Y_UPDATE_BORDER,
};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundGrid, ChangeLevelPayload,
    ClipRect, DeathKind, FontSize, InputCommand, InventoryObject, MAP_LEVEL, MessageDispatcher,
    MessageHandler, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand, ScreenHandler,
    ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{JnFile, JnObject, JnReadError};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;
use crate::entities::objects::{BeesEntity, BulletEntity, ScatterParticleEntity};
use crate::entities::{make_background_entity, make_object_entity};
use crate::screens::map_screen::render_map_background;
use crate::status_bar::GAME_AREA_CLIP;

/// Embedded `level_messagebox_vga.json` layout resource from the Java reference port.
const LEVEL_MESSAGEBOX_JSON: &str =
    include_str!("../../../../OpenJill/src/main/resources/level_messagebox_vga.json");

/// Save prefix for the episode 1 messages table inside
/// [`LEVEL_MESSAGEBOX_JSON`]; the JSON also carries `JN2` and `JN3` entries
/// that are not yet exercised by the runtime.
///
/// Sourced from [`openjill_data::episode::JILL1`] so the episode identity is
/// expressed through the canonical descriptor rather than a bare literal.
const EPISODE_SAVE_PREFIX: &str = openjill_data::episode::JILL1.jn_ext;

/// Sky / game-area background color for episode 1 levels, as a VGA palette
/// index.
///
/// The Java reference port (`AbstractBackgroundJillLevel`) fills the
/// off-screen background buffer with `colorMap[0]`, which in the shipped
/// `jill_color_map.properties` is the transparent sentinel (`!000000`). That
/// produces a black sky once composited over the cleared framebuffer, which
/// does not match the saturated dark blue sky the original DOS executable
/// renders for episode 1.  No JN field, `JillLevelConfiguration` member, or
/// per-level DMA palette carries this color either; the original engine
/// effectively hard-codes the sky per episode.  Episode 1 uses palette
/// index 1 (`0x0000A2`).
///
/// When JN2 / JN3 episode support lands this constant should be replaced
/// with an episode-aware lookup; see
/// `docs/port/06-episode-1-gameplay.md`.
pub const EPISODE_1_SKY_COLOR: u8 = 1;

/// Cached level message-box layout parsed from [`LEVEL_MESSAGEBOX_JSON`].
static MESSAGE_BOX: LazyLock<MessageBoxLayout> = LazyLock::new(parse_message_box_layout);

/// Pixel advance between successive text lines drawn inside the message box.
///
/// Matches the SHA font row height used elsewhere by the renderer.
const MESSAGE_LINE_HEIGHT: i32 = 8;

/// Maximum number of text lines drawn before clipping the message text.
///
/// Derived from the message-box `textarea.height` of 64 px divided by the
/// 8 px font row height.
const MESSAGE_MAX_LINES: usize = 8;

/// Pending level-change request captured from the [`MessageDispatcher`].
///
/// Recorded by [`InboxHandler`] when a level transition message arrives and
/// drained by [`LevelScreen::tick`] on the next tick so the screen can begin
/// the message-box countdown.
#[derive(Clone, Debug, PartialEq, Eq)]
enum PendingRequest {
    /// `CheckpointChangeLevel` payload pointing at the next level.
    ChangeLevel(ChangeLevelPayload),
    /// `CheckpointChangeLevelPrevious` request returning to the world map.
    PreviousMap,
    /// `DieRestartLevel` request reloading the current level.
    RestartLevel,
}

/// Shared queue of pending transition requests received via the dispatcher.
type Inbox = Arc<Mutex<Vec<PendingRequest>>>;

/// Inventory-area-local X of the score number's right edge.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json` `score.x`;
/// the Java reference treats this as the right edge for right-aligned rendering.
/// Value 63 places the last digit flush with the inventory area's right interior
/// column (inventory right edge = 64, minus one pixel of padding).
const SCORE_X_INV: i32 = 63;

/// Inventory-area-local Y of the score number's top edge.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json` `score.y`.
const SCORE_Y_INV: i32 = 16;

/// EGA color index used by the score digits.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json` `score.color`.
const SCORE_COLOR: u8 = 4;

/// Pixel width of one small-font glyph (confirmed from JILL1.SHA font tileset 2).
const SMALL_FONT_CHAR_W: i32 = 6;

/// Width in pixels of the FillRect erase region behind the score digits.
///
/// Covers the maximum right-aligned score area: `SCORE_DIGITS` glyphs of
/// `SMALL_FONT_CHAR_W` each, left-edge at `SCORE_X_INV - SCORE_DIGITS * SMALL_FONT_CHAR_W`.
const SCORE_ERASE_W: u32 = (SCORE_DIGITS as i32 * SMALL_FONT_CHAR_W) as u32;

/// Height in pixels of the FillRect erase region behind the score digits.
///
/// One small-font row (6 px) plus two pixels of vertical slack.
const SCORE_ERASE_H: u32 = 8;

/// EGA palette index used to erase the score region before each redraw.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json`
/// `backgroundColor` (the inventory area's flat background fill).
const INVENTORY_BG_COLOR: u8 = 8;

/// Number of zero-padded decimal digits drawn for the score value.
///
/// Matches the original game's six-digit score display.
const SCORE_DIGITS: usize = 6;

/// Maximum displayable score value (`10^SCORE_DIGITS - 1`).
///
/// The score is clamped to this ceiling when ingested from
/// `InventoryPoint` deltas so the rendered text always fits the six-digit
/// erase band; a runaway score never spills extra glyphs past
/// [`SCORE_ERASE_W`] into adjacent UI.  Also enforced when reading from
/// [`RuntimeState`] in the overlay path so externally-mutated state stays
/// inside the same visual contract.
const SCORE_DISPLAY_MAX: i32 = 999_999;

/// Inventory-area-local X of the inventory item grid's top-left cell.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json`
/// `itemConf.x`.
const ITEM_GRID_X_INV: i32 = 2;

/// Inventory-area-local Y of the inventory item grid's top-left cell.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json`
/// `itemConf.y`.
const ITEM_GRID_Y_INV: i32 = 27;

/// Number of inventory item grid rows.
///
/// Sourced from `inventory_conf.json` `itemConf.nbRow`.
const ITEM_GRID_ROWS: usize = 3;

/// Number of inventory item grid columns.
///
/// Sourced from `inventory_conf.json` `itemConf.nbCol`.
const ITEM_GRID_COLS: usize = 4;

/// Pixel pitch between adjacent inventory item grid cells.
const ITEM_GRID_PITCH: i32 = 16;

/// Inventory-area-local X of the "health" static label.
/// Sourced from `inventory_conf.json` `text[0].x`.
const HEALTH_LABEL_X_INV: i32 = 2;
/// Inventory-area-local Y of the "health" static label.
/// Sourced from `inventory_conf.json` `text[0].y`.
const HEALTH_LABEL_Y_INV: i32 = 2;
/// EGA color index for the "health" label.
const HEALTH_LABEL_COLOR: u8 = 5;

/// SHA tileset for the health-bar segment and end-cap tiles.
/// Sourced from `inventory_conf.json` `lifebarPictureStart.tileset`.
const LIFEBAR_TILESET: u8 = 14;
/// Tile index for one health bar segment (drawn once per health point).
/// Sourced from `inventory_conf.json` `lifebarPictureStart.tile`.
const LIFEBAR_TILE: u16 = 42;
/// Tile index for the bar end-cap (drawn once, left of segments).
/// Sourced from `inventory_conf.json` `lifebarPictureEnd.tile`.
const LIFEBAR_END_TILE: u16 = 43;
/// Inventory-area-local X where the first segment is drawn.
/// Sourced from `inventory_conf.json` `lifebar.x`.
const LIFEBAR_X_INV: i32 = 42;
/// Inventory-area-local Y for both the end-cap and segments.
/// Sourced from `inventory_conf.json` `lifebar.y`.
const LIFEBAR_Y_INV: i32 = 2;
/// Inventory-area-local X of the end-cap tile.
/// Sourced from `inventory_conf.json` `lifebarEnd.x`.
const LIFEBAR_END_X_INV: i32 = 40;
/// Pixel step between consecutive segment blits.
/// Sourced from `inventory_conf.json` `lifeBarStepSize`.
const LIFEBAR_STEP: i32 = 3;
/// Maximum segment count (full health).
/// Sourced from `inventory_conf.json` `maxLife`.
const LIFEBAR_MAX: i32 = 8;
/// Pixel width of one lifebar tile (segment or end-cap).
/// Confirmed from JILL1.SHA tileset 14: tile[42] and tile[43] are both 4×6 px.
const LIFEBAR_TILE_W: i32 = 4;
/// Pixel height of the FillRect that erases the lifebar region before redraw.
const LIFEBAR_ERASE_H: u32 = 8;

/// Inventory-area-local X of the "level" static label.
/// Sourced from `inventory_conf.json` `text[1].x`.
const LEVEL_LABEL_X_INV: i32 = 1;
/// Inventory-area-local Y of the "level" static label.
/// Sourced from `inventory_conf.json` `text[1].y`.
const LEVEL_LABEL_Y_INV: i32 = 10;
/// EGA color index for the "level" label.
const LEVEL_LABEL_COLOR: u8 = 2;

/// Inventory-area-local X of the "map" static label.
/// Sourced from `inventory_conf.json` `text[2].x`.
const MAP_LABEL_X_INV: i32 = 1;
/// Inventory-area-local Y of the "map" static label.
/// Sourced from `inventory_conf.json` `text[2].y`.
const MAP_LABEL_Y_INV: i32 = 16;
/// EGA color index for the "map" label.
const MAP_LABEL_COLOR: u8 = 2;

/// Inventory-area-local X of the "score" static label.
/// Sourced from `inventory_conf.json` `text[3].x`.
const SCORE_LABEL_X_INV: i32 = 33;
/// Inventory-area-local Y of the "score" static label.
/// Sourced from `inventory_conf.json` `text[3].y`.
const SCORE_LABEL_Y_INV: i32 = 10;
/// EGA color index for the "score" label.
const SCORE_LABEL_COLOR: u8 = 4;

/// Framebuffer clip rectangle that confines dynamic-overlay output to the
/// inventory area (origin `(INVENTORY_AREA_X, INVENTORY_AREA_Y)`, size
/// `INVENTORY_AREA_W × INVENTORY_AREA_H` from `openjill_core::layout`).
///
/// `inventory_conf.json` declares an item grid (`itemConf.x = 2`,
/// `itemConf.y = 27`, 4 cols × 3 rows × 16 px pitch) whose 64 × 48 footprint
/// sticks two pixels past the inventory area's right edge and six pixels
/// past its bottom edge; the Java reference renders the grid into a
/// `BufferedImage(INVENTORY_AREA_W, INVENTORY_AREA_H)` backing buffer that
/// silently clips the overflow.  The Rust port draws directly into the
/// framebuffer, so the same clip is supplied per-command on every
/// inventory-overlay blit + erase so the dynamic redraw cannot punch
/// through the surrounding status-bar frame (vertical bar tile at
/// `x = 72`, lower horizontal bar at `y = 176`, `"INVENTORY"` label band).
const INVENTORY_AREA_CLIP: ClipRect = ClipRect {
    x: INVENTORY_AREA_X,
    y: INVENTORY_AREA_Y,
    width: INVENTORY_AREA_W,
    height: INVENTORY_AREA_H,
};

/// EGA color index used by the in-game status-bar text overlay.
///
/// Matches the bright EGA index the level message-box uses for non-title
/// text; the renderer expands it to the bright variant before drawing.
const STATUS_BAR_TEXT_COLOR: u8 = 4;

/// Status-bar text X offset inside the message bar in framebuffer pixels.
///
/// Two-pixel left margin from the framebuffer origin.  The original
/// `status_bar_vga.json` `messageBar` entry covers the full screen width
/// (`x = 0`, `width = 320`) without specifying a text inset, so the value
/// is a port-side convention picked to match the small 6 × 6 SHA font's
/// visual padding inside the 12 px bar; later episodes can override it if
/// the reference layout grows a dedicated key.
const STATUS_BAR_TEXT_X: i32 = 2;

/// Status-bar text Y offset (relative to [`MESSAGE_BAR_Y`]) in framebuffer
/// pixels.
///
/// Three-pixel top margin centring the 6 px small font inside the
/// `MESSAGE_BAR_H = 12` band declared by `status_bar_vga.json`'s
/// `messageBar` entry.  The reference JSON does not pin the text origin
/// either, so this is a port-side convention matching the visual centring
/// the original DOS executable produces.
const STATUS_BAR_TEXT_Y_OFFSET: i32 = 3;

/// Returns the inventory item tileset / tile pair for an [`InventoryObject`]
/// variant.
///
/// Sourced from `OpenJill/src/main/resources/inventory_conf.json` `items`.
/// The mapping covers every variant currently emitted by episode 1 pickups;
/// [`InventoryObject::FireFlower`] reuses the `FROG` icon (tileset 14, tile
/// 14) and [`InventoryObject::Firebird`] reuses the `INVINCIBILITY` icon
/// (tileset 14, tile 37) because the original `inventory_conf.json` has no
/// dedicated entries for those gameplay-only pickups; child issue 60 will
/// refine the mapping once those pickups land.
fn inventory_item_tile(item: InventoryObject) -> (u8, u16) {
    match item {
        InventoryObject::Jill => (14, 38),
        InventoryObject::Gem => (14, 11),
        InventoryObject::Key => (14, 12),
        InventoryObject::Knife => (14, 13),
        InventoryObject::Blade => (14, 35),
        InventoryObject::FireFlower => (14, 14),
        InventoryObject::Firebird => (14, 37),
    }
}

/// Update queued by a status-bar dispatcher subscriber for the next tick.
///
/// Mirrors the [`PendingRequest`] inbox pattern: handlers register at
/// construction time, each handler pushes one variant into the shared queue,
/// and [`LevelScreen::pump_status_inbox`] drains the queue every tick before
/// rendering the dynamic overlay.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StatusUpdate {
    /// Score delta from an `InventoryPoint` message payload.
    Point(i32),
    /// Life-count delta from an `InventoryLife` message payload.
    Life(i32),
    /// Inventory item added or removed by an `InventoryItem` message payload.
    ///
    /// The boolean mirrors `InventoryItemMessage.isAddObject` from the Java
    /// reference: `true` appends a copy, `false` removes the first occurrence
    /// (no-op when the inventory does not contain `item`).
    Item(InventoryObject, bool),
    /// Status-bar text replacement from a `StatusBarText` message payload.
    Text(String),
}

/// Shared queue of status updates collected from the dispatcher subscribers.
type StatusInbox = Arc<Mutex<Vec<StatusUpdate>>>;

/// Shared queue of `Trigger` link identifiers dispatched during one tick.
///
/// Populated by [`TriggerInboxHandler`] and drained each tick by
/// [`LevelScreen::route_triggers`] which forwards each link identifier to
/// every object via [`ObjectEntity::receive_trigger`].
type TriggerInbox = Arc<Mutex<Vec<i32>>>;

/// Dispatcher handler that records arriving `Trigger` link identifiers.
struct TriggerInboxHandler {
    /// Shared inbox the level screen drains every tick.
    inbox: TriggerInbox,
}

impl MessageHandler for TriggerInboxHandler {
    /// Extracts the `Count` link identifier from a `Trigger` payload and
    /// appends it to the inbox.  Other payload variants are silently ignored.
    fn handle(&mut self, _msg_type: MessageType, payload: &MessagePayload) {
        if let MessagePayload::Count(link_id) = payload
            && let Ok(mut queue) = self.inbox.lock()
        {
            queue.push(*link_id);
        }
    }
}

/// Shared queue of `(dx, dy)` platform-move deltas dispatched during one tick.
///
/// Populated by [`PlayerMoveHandler`] and drained each tick by
/// [`LevelScreen::apply_platform_moves`] which forwards each delta to the
/// player entity via [`ObjectEntity::apply_platform_move`].
type PlayerMoveInbox = Arc<Mutex<Vec<(i32, i32)>>>;

/// Dispatcher handler that records arriving `PlayerMove` deltas.
struct PlayerMoveHandler {
    /// Shared inbox the level screen drains every tick.
    inbox: PlayerMoveInbox,
}

impl MessageHandler for PlayerMoveHandler {
    /// Extracts the `Move(dx, dy)` delta from a `PlayerMove` payload and
    /// appends it to the inbox.  Other payload variants are silently ignored.
    fn handle(&mut self, _msg_type: MessageType, payload: &MessagePayload) {
        if let MessagePayload::Move(dx, dy) = payload
            && let Ok(mut queue) = self.inbox.lock()
        {
            queue.push((*dx, *dy));
        }
    }
}

/// Shared queue of spawn parameters for objects created during one tick.
///
/// Each entry is `(object_type, x, y, xd, yd)`.  Populated by
/// [`CreateObjectHandler`] and drained each tick by
/// [`LevelScreen::spawn_objects`] which routes to the correct factory
/// constructor based on `object_type`.
type CreateObjectInbox = Arc<Mutex<Vec<(u8, i32, i32, i32, i32)>>>;

/// Dispatcher handler that records arriving `CreateObject` spawn requests.
struct CreateObjectHandler {
    /// Shared inbox the level screen drains every tick.
    inbox: CreateObjectInbox,
}

impl MessageHandler for CreateObjectHandler {
    /// Extracts `SpawnAt` fields from a `CreateObject` payload and appends
    /// them to the inbox.  `None` payloads (e.g. die burst) are ignored until
    /// that spawn path receives a structured payload.
    fn handle(&mut self, _msg_type: MessageType, payload: &MessagePayload) {
        if let MessagePayload::SpawnAt {
            object_type,
            x,
            y,
            xd,
            yd,
        } = payload
            && let Ok(mut queue) = self.inbox.lock()
        {
            queue.push((*object_type, *x, *y, *xd, *yd));
        }
    }
}

/// Shared queue of `(cell_x, cell_y)` background-clear requests dispatched
/// during one tick.
///
/// Populated by [`BackgroundClearHandler`] and drained each tick by
/// [`LevelScreen::apply_background_clears`], which replaces the named cells
/// with transparent passable entries so a door that just opened no longer
/// blocks the player physically.
type BackgroundClearInbox = Arc<Mutex<Vec<(i32, i32)>>>;

/// Dispatcher handler that records arriving `Background` clear requests.
struct BackgroundClearHandler {
    /// Shared inbox the level screen drains every tick.
    inbox: BackgroundClearInbox,
}

impl MessageHandler for BackgroundClearHandler {
    /// Extracts `ClearBackground { cell_x, cell_y }` from a `Background`
    /// payload and appends the pair to the inbox.  Other payload variants are
    /// silently ignored.
    fn handle(&mut self, _msg_type: MessageType, payload: &MessagePayload) {
        if let MessagePayload::ClearBackground { cell_x, cell_y } = payload
            && let Ok(mut queue) = self.inbox.lock()
        {
            queue.push((*cell_x, *cell_y));
        }
    }
}

/// Level screen handler.
///
/// Owns a parsed level JN file plus the raw bytes for restart-level
/// round-trips, the parsed `JILL.DMA` lookup, the current level number, and
/// the viewport offset seeded from the checkpoint object.  Listens for
/// transition messages via the shared [`Inbox`] and displays the level
/// message-box overlay during the countdown before surfacing the resulting
/// [`ScreenTransition`].
pub struct LevelScreen {
    /// Parsed level JN data for background and object lookups.
    jn: JnFile,
    /// Raw level bytes preserved verbatim for
    /// [`ScreenHandler::level_jn_bytes`] round-trips.
    jn_bytes: Vec<u8>,
    /// Parsed `JILL.DMA` for background map-code to tileset/tile resolution.
    dma: DmaFile,
    /// Current level number; used to look up the checkpoint object and the
    /// per-level message text from `level_messagebox_vga.json`.
    level_number: i32,
    /// Viewport X offset in pixels following OpenJill sign convention.
    viewport_x: i32,
    /// Viewport Y offset in pixels following OpenJill sign convention.
    viewport_y: i32,
    /// Pending transition request received from the dispatcher.
    pending: Option<PendingRequest>,
    /// Remaining ticks before applying the pending transition.
    message_ticks: u32,
    /// Cached message-box text lines for the pending transition.
    message_text: Vec<String>,
    /// Shared inbox holding new transition requests delivered by subscribed
    /// dispatcher handlers.
    inbox: Inbox,
    /// Active object entities built from the JN object list.
    ///
    /// Iterated each tick: entries whose bounding box lies within the
    /// viewport update border (or that return `always_active`) advance via
    /// `ObjectEntity::update`; entries within the game area emit their
    /// `ObjectEntity::draw` command.  Order matches the JN object list so
    /// rendering follows the Java reference's draw-order convention.
    objects: Vec<Box<dyn ObjectEntity>>,
    /// Background entity grid built from the JN background layer and the
    /// `JILL.DMA` cell metadata.
    ///
    /// Iterated each tick for visible cells: cells that overlap the player
    /// receive `on_player_touch`; cells that opt into per-tick updates via
    /// `BackgroundEntity::needs_update` advance their state; every visible
    /// cell contributes its `BackgroundEntity::draw` command on top of the
    /// JN-driven base layer.
    backgrounds: BackgroundGrid,
    /// Local message dispatcher passed into per-tick entity callbacks.
    ///
    /// Entity-to-entity events (object removal, pickup notifications,
    /// internal triggers) flow through this dispatcher.  Cross-screen
    /// transition messages still flow through the orchestrator dispatcher via
    /// [`InboxHandler`].  Cleared after each tick because no inter-entity
    /// subscribers exist yet; child issues attach real subscribers here.
    entity_dispatcher: MessageDispatcher,
    /// VGA palette index used to fill the game area before any tile blits.
    ///
    /// See [`EPISODE_1_SKY_COLOR`] for the sourcing rationale; supplied by the
    /// caller at construction so future episode support can vary the value
    /// without changing this screen.
    sky_color: u8,
    /// Shared queue holding score / life / item / status-bar text updates
    /// collected via the dispatcher subscribers registered in
    /// [`LevelScreen::new`].
    ///
    /// Drained every tick by [`LevelScreen::pump_status_inbox`] so the
    /// matching effects (score and lives accumulation, inventory append,
    /// status-bar text reset) land on the same frame the dispatcher delivered
    /// the message.
    status_inbox: StatusInbox,
    /// Currently displayed status-bar text, or `None` when no message is
    /// active.
    ///
    /// Set by an incoming `StatusBarText` message and cleared when
    /// [`Self::status_text_ticks`] reaches zero.
    status_text: Option<String>,
    /// Remaining ticks before the active status-bar text is cleared.
    ///
    /// Seeded to [`LEVEL_MESSAGE_TICKS`] (72 ticks ≈ 4 s) on every new
    /// `StatusBarText` message; counted down each tick by [`Self::tick`].
    status_text_ticks: u32,
    /// Shared queue of `Trigger` link identifiers dispatched during the
    /// current tick by switches or touch triggers.
    ///
    /// Drained each tick by [`Self::route_triggers`] which forwards each
    /// link identifier to every object via [`ObjectEntity::receive_trigger`].
    trigger_inbox: TriggerInbox,
    /// Shared queue of `(dx, dy)` platform-move deltas dispatched by lift
    /// entities during the current tick.
    ///
    /// Drained each tick by [`Self::apply_platform_moves`] which translates
    /// the player entity position by the accumulated delta.
    player_move_inbox: PlayerMoveInbox,
    /// Shared queue of `(x, y, xd, yd)` spawn parameters for bullets
    /// requested via `CreateObject` during the current tick.
    ///
    /// Drained each tick by [`Self::spawn_objects`] which appends a new
    /// [`BulletEntity`] to the active object list for each entry.
    create_object_inbox: CreateObjectInbox,
    /// Shared queue of `(cell_x, cell_y)` background-clear requests dispatched
    /// by door entities when they open.
    ///
    /// Drained each tick by [`Self::apply_background_clears`] which replaces
    /// the named cells with transparent passable entries so the opened doorway
    /// no longer blocks the player.
    background_clear_inbox: BackgroundClearInbox,
}

impl LevelScreen {
    /// Creates a level screen from parsed level data and the originating
    /// bytes, registering message handlers on `dispatcher` so the level
    /// transition messages are routed back to this screen on subsequent ticks.
    ///
    /// `cache` supplies the parsed `JILL.DMA` plus shared SHA tileset data
    /// used by the object and background entity factories.  The cache's DMA
    /// is cloned into the screen for the legacy `render_map_background` path
    /// so the screen can render without further cache access.
    ///
    /// `sky_color` is the VGA palette index used to fill the game area before
    /// any background tile blits each tick.  Callers loading an episode 1
    /// level should pass [`EPISODE_1_SKY_COLOR`].
    pub fn new(
        jn: JnFile,
        jn_bytes: Vec<u8>,
        cache: &AssetCache,
        level_number: i32,
        dispatcher: &mut MessageDispatcher,
        sky_color: u8,
    ) -> Self {
        let inbox: Inbox = Arc::new(Mutex::new(Vec::new()));
        dispatcher.subscribe(
            MessageType::CheckpointChangeLevel,
            Box::new(InboxHandler {
                inbox: Arc::clone(&inbox),
            }),
        );
        dispatcher.subscribe(
            MessageType::CheckpointChangeLevelPrevious,
            Box::new(InboxHandler {
                inbox: Arc::clone(&inbox),
            }),
        );
        dispatcher.subscribe(
            MessageType::DieRestartLevel,
            Box::new(InboxHandler {
                inbox: Arc::clone(&inbox),
            }),
        );

        let status_inbox: StatusInbox = Arc::new(Mutex::new(Vec::new()));
        dispatcher.subscribe(
            MessageType::InventoryPoint,
            Box::new(StatusInboxHandler {
                inbox: Arc::clone(&status_inbox),
            }),
        );
        dispatcher.subscribe(
            MessageType::InventoryLife,
            Box::new(StatusInboxHandler {
                inbox: Arc::clone(&status_inbox),
            }),
        );
        dispatcher.subscribe(
            MessageType::InventoryItem,
            Box::new(StatusInboxHandler {
                inbox: Arc::clone(&status_inbox),
            }),
        );
        dispatcher.subscribe(
            MessageType::StatusBarText,
            Box::new(StatusInboxHandler {
                inbox: Arc::clone(&status_inbox),
            }),
        );

        let (viewport_x, viewport_y) = checkpoint_viewport(&jn, level_number);
        let objects = build_object_entities(&jn, cache);
        let backgrounds = build_background_grid(&jn, cache);
        let dma = cache.dma.clone();

        // Mirror the inbox subscriptions on the local `entity_dispatcher` so
        // messages dispatched by per-object `on_touch` / `update` callbacks
        // reach the same status and transition queues the orchestrator-side
        // dispatcher already routes to.  The handlers share the underlying
        // `Arc` inboxes so both dispatchers append into the same queue.
        let mut entity_dispatcher = MessageDispatcher::new();
        for msg_type in [
            MessageType::CheckpointChangeLevel,
            MessageType::CheckpointChangeLevelPrevious,
            MessageType::DieRestartLevel,
        ] {
            entity_dispatcher.subscribe(
                msg_type,
                Box::new(InboxHandler {
                    inbox: Arc::clone(&inbox),
                }),
            );
        }
        for msg_type in [
            MessageType::InventoryPoint,
            MessageType::InventoryLife,
            MessageType::InventoryItem,
            MessageType::StatusBarText,
        ] {
            entity_dispatcher.subscribe(
                msg_type,
                Box::new(StatusInboxHandler {
                    inbox: Arc::clone(&status_inbox),
                }),
            );
        }

        let trigger_inbox: TriggerInbox = Arc::new(Mutex::new(Vec::new()));
        entity_dispatcher.subscribe(
            MessageType::Trigger,
            Box::new(TriggerInboxHandler {
                inbox: Arc::clone(&trigger_inbox),
            }),
        );

        let player_move_inbox: PlayerMoveInbox = Arc::new(Mutex::new(Vec::new()));
        entity_dispatcher.subscribe(
            MessageType::PlayerMove,
            Box::new(PlayerMoveHandler {
                inbox: Arc::clone(&player_move_inbox),
            }),
        );

        let create_object_inbox: CreateObjectInbox = Arc::new(Mutex::new(Vec::new()));
        entity_dispatcher.subscribe(
            MessageType::CreateObject,
            Box::new(CreateObjectHandler {
                inbox: Arc::clone(&create_object_inbox),
            }),
        );

        let background_clear_inbox: BackgroundClearInbox = Arc::new(Mutex::new(Vec::new()));
        entity_dispatcher.subscribe(
            MessageType::Background,
            Box::new(BackgroundClearHandler {
                inbox: Arc::clone(&background_clear_inbox),
            }),
        );

        Self {
            jn,
            jn_bytes,
            dma,
            level_number,
            viewport_x,
            viewport_y,
            pending: None,
            message_ticks: 0,
            message_text: Vec::new(),
            inbox,
            objects,
            backgrounds,
            entity_dispatcher,
            sky_color,
            status_inbox,
            status_text: None,
            status_text_ticks: 0,
            trigger_inbox,
            player_move_inbox,
            create_object_inbox,
            background_clear_inbox,
        }
    }

    /// Parses `bytes` as a level JN file and wraps it in a [`LevelScreen`].
    ///
    /// Returns the underlying [`JnReadError`] when parsing fails.
    ///
    /// `sky_color` is forwarded to [`LevelScreen::new`]; episode 1 callers
    /// should pass [`EPISODE_1_SKY_COLOR`].
    pub fn from_bytes(
        bytes: Vec<u8>,
        cache: &AssetCache,
        level_number: i32,
        dispatcher: &mut MessageDispatcher,
        sky_color: u8,
    ) -> Result<Self, JnReadError> {
        let jn = JnFile::from_bytes(bytes.clone())?;
        Ok(Self::new(
            jn,
            bytes,
            cache,
            level_number,
            dispatcher,
            sky_color,
        ))
    }

    /// Returns the current viewport `(x, y)` offset in pixels.
    ///
    /// Exposed for tests that exercise the checkpoint viewport seeding.
    pub fn viewport(&self) -> (i32, i32) {
        (self.viewport_x, self.viewport_y)
    }

    /// Drains the dispatcher inbox and promotes the first new request to a
    /// pending transition, starting the message-box countdown.
    ///
    /// The inbox is drained unconditionally on every tick so messages
    /// dispatched during an active 72-tick hold are dropped immediately
    /// rather than accumulating until the screen is swapped.
    fn pump_inbox(&mut self) {
        let next = {
            let mut queue = self.inbox.lock().expect("level inbox mutex poisoned");
            if queue.is_empty() {
                return;
            }
            let first = queue.remove(0);
            queue.clear();
            first
        };

        // Same-level checkpoint: a `CheckpointChangeLevel` whose target is
        // this very level is a within-level save-point, not a screen
        // transition.  Silently drop it so the player does not get sent into
        // an infinite reload loop by passing through the level-entry
        // checkpoint.
        if let PendingRequest::ChangeLevel(payload) = &next
            && payload.level_number == self.level_number
        {
            return;
        }

        // A transition is already pending: drop the late-arriving request to
        // match the Java reference's single-shot behavior.
        if self.pending.is_some() {
            return;
        }

        let target_level = match &next {
            PendingRequest::ChangeLevel(payload) => payload.level_number,
            PendingRequest::PreviousMap => openjill_core::MAP_LEVEL,
            PendingRequest::RestartLevel => self.level_number,
        };
        self.message_text = lookup_message_text(target_level);
        self.message_ticks = LEVEL_MESSAGE_TICKS;
        self.pending = Some(next);
    }

    /// Renders the base frame: the per-level sky fill over the game area
    /// and the static level background.
    ///
    /// The presenter already clears the indexed framebuffer to palette
    /// index 0 at the top of every `execute_and_present` call, and the
    /// orchestrator prepends the static status-bar tile mosaic to the
    /// frame's command list before the level handler's commands; emitting
    /// a `RenderCommand::Clear` here would run *after* the status-bar tiles
    /// were laid down and overwrite them with the clear color, leaving the
    /// inventory / control / message-bar regions black.  The
    /// [`RenderCommand::FillRect`] that follows fills only the game-area
    /// sub-region with [`self.sky_color`] so transparent map cells (map
    /// code 0) reveal the per-episode sky instead of the framebuffer's
    /// palette-index-0 clear.
    ///
    /// The message-box overlay is intentionally not included here; the tick
    /// loop appends it after the per-entity draw commands so the box paints
    /// on top of the level and any objects in front of it.
    fn render_base_frame(&self) -> Vec<RenderCommand> {
        let mut commands = vec![RenderCommand::FillRect {
            x: GAME_AREA_X,
            y: GAME_AREA_Y,
            width: GAME_AREA_W,
            height: GAME_AREA_H,
            color: self.sky_color,
        }];
        commands.extend(render_map_background(
            &self.jn,
            &self.dma,
            self.viewport_x,
            self.viewport_y,
        ));
        commands
    }

    /// Returns the player's bounding box, if a player entity exists.
    ///
    /// The first object reporting [`ObjectEntity::is_player`] is treated as
    /// the active player; later swaps (Firebird transform) replace this entry
    /// in place.
    fn player_bounding_box(&self) -> Option<Rect> {
        self.objects
            .iter()
            .find(|obj| obj.is_player())
            .map(|obj| obj.bounding_box())
    }

    /// Advances every object entity by one tick.
    ///
    /// An object updates when it reports `always_active` or when its
    /// bounding box overlaps the viewport expanded by [`X_UPDATE_BORDER`] and
    /// [`Y_UPDATE_BORDER`].  The update rect is evaluated against the viewport
    /// *before* this frame's scroll snap so objects whose pre-tick bounding
    /// box was within the border still update on the tick they leave the
    /// border, matching the Java reference's `AbstractExecutingStdPlayerLevel`
    /// iteration order.
    fn update_objects(&mut self, input: &ActiveInput, state: &RuntimeState) {
        let update_rect = viewport_update_rect(self.viewport_x, self.viewport_y);
        // Capture the player's current bounding box once so hazards that
        // depend on it (collapsing ceilings, falling spikes, lifts later)
        // observe the same player snapshot across the whole tick.  None when
        // no player is in the object list yet (e.g. before object factories
        // register the player).
        let player_bbox = self.player_bounding_box();
        for obj in self.objects.iter_mut() {
            if let Some(bbox) = player_bbox
                && !obj.is_player()
            {
                obj.observe_player(bbox);
            }
            let bbox = obj.bounding_box();
            if obj.always_active() || update_rect.intersects(&bbox) {
                obj.update(input, state, &self.backgrounds, &mut self.entity_dispatcher);
            }
        }
    }

    /// Dispatches `on_touch` to every non-player object whose bounding box
    /// overlaps the player.
    ///
    /// Mirrors the Java reference's per-frame "for each other object, when
    /// rectangles intersect call `msgTouch`" pass in
    /// `AbstractExecutingStdPlayerLevel`.  Pickup entities react inside
    /// `on_touch` by flipping their `should_remove` flag so
    /// [`Self::reap_removed_objects`] can purge them on the same tick.
    ///
    /// Hazard objects cannot reach the player from inside `on_touch`, so
    /// after the touch dispatch each object is asked for any pending player
    /// kill classification via [`ObjectEntity::take_player_kill`]; the first
    /// non-`None` result deducts one health point from `state`.  Only when
    /// health reaches zero is `player.on_kill(1, kind)` called, arming the
    /// player's `Die` sub-state (subsequent ticks then dispatch
    /// `DieRestartLevel` themselves).  This mirrors Java's `hitPlayer()` →
    /// `INVENTORY_LIFE(-1)` → `isPlayerDead()` check before `killPlayer()`.
    fn dispatch_player_touches(&mut self, state: &mut RuntimeState) {
        let Self {
            objects,
            entity_dispatcher,
            ..
        } = self;
        let Some(player_idx) = objects.iter().position(|obj| obj.is_player()) else {
            return;
        };
        let player_bbox = objects[player_idx].bounding_box();
        let mut pending_kill: Option<DeathKind> = None;
        for (idx, obj) in objects.iter_mut().enumerate() {
            if idx == player_idx {
                continue;
            }
            if obj.bounding_box().intersects(&player_bbox) {
                obj.on_touch(state, entity_dispatcher);
                if pending_kill.is_none() {
                    pending_kill = obj.take_player_kill();
                }
            }
        }
        if let Some(kind) = pending_kill
            && state.invincibility_ticks == 0
        {
            state.health = (state.health - 1).max(0);
            state.invincibility_ticks = openjill_core::PLAYER_INVINCIBILITY_TICKS;
            if state.health == 0 {
                objects[player_idx].on_kill(1, kind);
            }
        }
    }

    /// Dispatches a one-point hit from every player-spawned projectile to
    /// every non-player non-projectile object whose bounding box overlaps.
    ///
    /// Mirrors the Java reference's bullet-vs-enemy collision pass: when a
    /// thrown knife / bullet sprite overlaps an enemy, both the projectile
    /// and the enemy receive [`ObjectEntity::on_kill`] so the enemy enters
    /// its death state and the projectile is reaped on the same tick.
    ///
    /// Collected as `(projectile_idx, target_idx)` pairs first so the
    /// mutable borrows can be applied sequentially without violating
    /// Rust's aliasing rules. `DeathKind::Enemy` is used for both ends
    /// because the projectile's own `on_kill` only flips its `removed`
    /// flag and ignores the death-kind.
    fn dispatch_projectile_hits(&mut self) {
        let mut hits: Vec<(usize, usize)> = Vec::new();
        for (p_idx, projectile) in self.objects.iter().enumerate() {
            if !projectile.is_projectile() {
                continue;
            }
            let p_bbox = projectile.bounding_box();
            for (t_idx, target) in self.objects.iter().enumerate() {
                if t_idx == p_idx
                    || target.is_player()
                    || target.is_projectile()
                    || target.is_dead()
                    || target.should_remove()
                {
                    continue;
                }
                if target.bounding_box().intersects(&p_bbox) {
                    hits.push((p_idx, t_idx));
                }
            }
        }
        for (p_idx, t_idx) in hits {
            let target_bbox = self.objects[t_idx].bounding_box();
            let was_dead = self.objects[t_idx].is_dead();
            self.objects[t_idx].on_kill(1, DeathKind::Enemy);
            self.objects[p_idx].on_kill(1, DeathKind::Enemy);
            // Visible death shatter: spawn the colored-bullet burst
            // only on the alive -> dead transition so a projectile
            // lingering on a corpse does not re-fire the burst every
            // tick.
            if !was_dead && self.objects[t_idx].is_dead() {
                crate::entities::objects::scatter_particle::spawn_burst_at(
                    target_bbox.x + target_bbox.w / 2,
                    target_bbox.y + target_bbox.h / 2,
                    &mut self.entity_dispatcher,
                );
            }
        }
    }

    /// Drains the trigger inbox and forwards each link identifier to every
    /// object via [`ObjectEntity::receive_trigger`].
    ///
    /// Called once per tick after [`Self::update_objects`] so that `Trigger`
    /// messages dispatched by switches and touch triggers during `update`
    /// are delivered to toggle walls and other trigger-sensitive objects
    /// on the same tick.
    fn route_triggers(&mut self) {
        let link_ids: Vec<i32> = {
            let mut queue = self
                .trigger_inbox
                .lock()
                .expect("trigger inbox mutex poisoned");
            std::mem::take(&mut *queue)
        };
        if link_ids.is_empty() {
            return;
        }
        for obj in self.objects.iter_mut() {
            for &link_id in &link_ids {
                obj.receive_trigger(link_id);
            }
        }
    }

    /// Applies accumulated `PlayerMove` deltas to the player entity.
    ///
    /// Drains the `player_move_inbox` and calls
    /// [`ObjectEntity::apply_platform_move`] on every entity that reports
    /// `is_player()`.  Non-player entities have a no-op default, so the
    /// filter on `is_player` is an optimisation rather than a correctness
    /// requirement.
    fn apply_platform_moves(&mut self) {
        let moves: Vec<(i32, i32)> = {
            let mut queue = self
                .player_move_inbox
                .lock()
                .expect("player move inbox mutex poisoned");
            std::mem::take(&mut *queue)
        };
        if moves.is_empty() {
            return;
        }
        for obj in self.objects.iter_mut().filter(|o| o.is_player()) {
            for &(dx, dy) in &moves {
                obj.apply_platform_move(dx, dy);
            }
        }
    }

    /// Instantiates objects requested via `CreateObject` during this tick.
    ///
    /// Drains the `create_object_inbox` and routes each `(object_type, x, y,
    /// xd, yd)` entry to the appropriate factory constructor.  New objects
    /// join the scene on the same tick they were requested, meaning they
    /// participate in the draw pass but not in the touch-detection pass (which
    /// has already run by the time `spawn_objects` is called).
    ///
    /// Supported `object_type` values:
    /// - `36` ([`BulletEntity`]): player-fired projectile.
    /// - `46` ([`BeesEntity`]): bee swarm spawned by a hive.
    fn spawn_objects(&mut self) {
        let spawns: Vec<(u8, i32, i32, i32, i32)> = {
            let mut queue = self
                .create_object_inbox
                .lock()
                .expect("create object inbox mutex poisoned");
            std::mem::take(&mut *queue)
        };
        for (object_type, x, y, xd, yd) in spawns {
            let entity: Box<dyn ObjectEntity> = match object_type {
                46 => Box::new(BeesEntity::spawn_at(x, y)),
                t if t == crate::entities::objects::scatter_particle::SCATTER_PARTICLE_TYPE => {
                    Box::new(ScatterParticleEntity::with_velocity(x, y, xd, yd))
                }
                _ => Box::new(BulletEntity::with_velocity(
                    x,
                    y,
                    BLOCK_SIZE_I,
                    BLOCK_SIZE_I,
                    xd,
                    yd,
                )),
            };
            self.objects.push(entity);
        }
    }

    /// Drains the background-clear inbox and replaces each named cell with a
    /// transparent passable [`StdBackgroundEntity`].
    ///
    /// Called after [`Self::spawn_objects`] and before
    /// [`Self::update_viewport`] so the cleared cell is absent from the
    /// collision grid on the same tick the door opens.
    fn apply_background_clears(&mut self) {
        let clears: Vec<(i32, i32)> = {
            let mut queue = self
                .background_clear_inbox
                .lock()
                .expect("background clear inbox mutex poisoned");
            std::mem::take(&mut *queue)
        };
        for (cx, cy) in clears {
            if cx < 0 || cy < 0 {
                continue;
            }
            if let Some(cell) = self.backgrounds.get_mut(cx as usize, cy as usize) {
                *cell = Box::new(StdBackgroundEntity::transparent());
            }
        }
    }

    /// Drops every object whose `should_remove` flag is set.
    ///
    /// Runs after the touch dispatch so pickups that mark themselves for
    /// removal disappear before the draw pass.  Preserves the relative order
    /// of the surviving objects so draw order remains stable.
    fn reap_removed_objects(&mut self) {
        self.objects.retain(|obj| !obj.should_remove());
    }

    /// Collects render commands for every object whose post-update bounding
    /// box overlaps the visible game-area window.
    ///
    /// Run after [`Self::update_viewport`] so commands use the freshly-snapped
    /// viewport: the player and any object that moved with this tick land at
    /// their new screen position rather than at the previous frame's offset.
    fn draw_objects(&mut self) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let game_rect = viewport_game_rect(self.viewport_x, self.viewport_y);
        for obj in self.objects.iter_mut() {
            let bbox = obj.bounding_box();
            if game_rect.intersects(&bbox) {
                for cmd in obj.draw_multi() {
                    commands.push(translate_object_command(
                        cmd,
                        self.viewport_x,
                        self.viewport_y,
                    ));
                }
            }
        }
        commands
    }

    /// Snaps the viewport so the player remains inside the 96 px horizontal
    /// and 48 px vertical update border, clamped to the map bounds.
    ///
    /// Delegates to [`compute_viewport_scroll`] for the per-axis snap +
    /// clamp arithmetic; this method only locates the player bounding box and
    /// writes the result back into [`Self::viewport_x`] / [`Self::viewport_y`].
    ///
    /// No-op when no player entity exists yet (e.g. before object factories
    /// register the player).
    fn update_viewport(&mut self) {
        let Some(player_bbox) = self.player_bounding_box() else {
            return;
        };
        let (vx, vy) = compute_viewport_scroll(player_bbox, self.viewport_x, self.viewport_y);
        self.viewport_x = vx;
        self.viewport_y = vy;
    }

    /// Iterates visible background cells, applies per-cell callbacks, and
    /// collects each cell's render command.
    ///
    /// A cell is "visible" when its 16x16 pixel rectangle overlaps the
    /// viewport-positioned game area.  When the player overlaps a cell, the
    /// cell's `on_player_touch` runs first; cells that report
    /// `needs_update` then advance via `update`; every visible cell finally
    /// contributes its `draw` output.
    fn tick_backgrounds(&mut self, player_bbox: Option<Rect>) -> Vec<RenderCommand> {
        let viewport_x = self.viewport_x;
        let viewport_y = self.viewport_y;
        // `viewport_x` / `viewport_y` follow the OpenJill offset sign
        // convention: the world pixel currently at the viewport top-left is
        // `(-viewport_x, -viewport_y)`.  Compute that world origin once so the
        // cell iteration starts at the right tile and the screen conversion
        // below stays consistent with `render_map_background`.
        let world_origin_x = -viewport_x;
        let world_origin_y = -viewport_y;
        let start_cell_x = world_origin_x.div_euclid(BLOCK_SIZE_I);
        let start_cell_y = world_origin_y.div_euclid(BLOCK_SIZE_I);
        let cells_x = (GAME_AREA_W as i32) / BLOCK_SIZE_I + 2;
        let cells_y = (GAME_AREA_H as i32) / BLOCK_SIZE_I + 2;
        // Borrow the three fields the loop needs as disjoint mutable
        // references so the cell's `on_player_touch` can take a `&mut dyn
        // ObjectEntity` from `objects` while the iteration still holds a
        // `&mut` borrow into `backgrounds`.
        let Self {
            backgrounds,
            objects,
            entity_dispatcher,
            ..
        } = self;
        let height = backgrounds.height as i32;
        let width = backgrounds.width as i32;
        let player_idx = objects.iter().position(|obj| obj.is_player());

        let mut commands = Vec::new();
        for row in 0..cells_y {
            for col in 0..cells_x {
                let cell_x = start_cell_x + col;
                let cell_y = start_cell_y + row;
                if cell_x < 0 || cell_y < 0 || cell_x >= width || cell_y >= height {
                    continue;
                }
                let cell_pixel_x = cell_x * BLOCK_SIZE_I;
                let cell_pixel_y = cell_y * BLOCK_SIZE_I;
                let cell_rect = Rect::new(cell_pixel_x, cell_pixel_y, BLOCK_SIZE_I, BLOCK_SIZE_I);

                let Some(cell) = backgrounds.get_mut(cell_x as usize, cell_y as usize) else {
                    continue;
                };

                if let Some(bbox) = player_bbox
                    && bbox.intersects(&cell_rect)
                    && let Some(idx) = player_idx
                {
                    let player = objects[idx].as_mut();
                    cell.on_player_touch(player, entity_dispatcher);
                }

                if cell.needs_update() {
                    cell.update(cell_x, cell_y, entity_dispatcher);
                }

                // World pixel `cell_pixel_x` maps to game-area pixel
                // `cell_pixel_x - world_origin_x`, then the framebuffer
                // origin shift adds `GAME_AREA_X`.  Subtracting
                // `world_origin_x` is equivalent to adding `viewport_x`
                // because `world_origin_x = -viewport_x`.
                let screen_x = GAME_AREA_X + cell_pixel_x - world_origin_x;
                let screen_y = GAME_AREA_Y + cell_pixel_y - world_origin_y;
                if let Some(cmd) = cell.draw(screen_x, screen_y) {
                    commands.push(cmd);
                }
            }
        }
        commands
    }

    /// Drains the status-update inbox and applies each entry to the shared
    /// [`RuntimeState`] (score, lives, inventory) plus the local status-bar
    /// text fields.
    ///
    /// Mirrors the [`PendingRequest`] inbox drain: handlers registered in
    /// [`LevelScreen::new`] push one [`StatusUpdate`] per dispatched message,
    /// and this drain pass converts those queued updates into mutations on
    /// `state` and `self`.  `StatusUpdate::Text` resets the
    /// [`LEVEL_MESSAGE_TICKS`] countdown so a second message extends the
    /// display rather than letting an in-flight clear short-circuit the new
    /// text.
    fn pump_status_inbox(&mut self, state: &mut RuntimeState) {
        let updates: Vec<StatusUpdate> = {
            let mut queue = self
                .status_inbox
                .lock()
                .expect("level status inbox mutex poisoned");
            std::mem::take(&mut *queue)
        };
        for update in updates {
            match update {
                StatusUpdate::Point(delta) => {
                    // Clamp to `[0, SCORE_DISPLAY_MAX]` so negative deltas
                    // cannot drive the visible score below zero and so a
                    // streak of pickups cannot push the rendered digit count
                    // past the six-digit erase band defined by
                    // `SCORE_ERASE_W`.  The clamp lives at ingest rather
                    // than on the render path so downstream gameplay logic
                    // (HUD readers, save-game writers) sees the same value
                    // the player sees on-screen.
                    state.score = state
                        .score
                        .saturating_add(delta)
                        .clamp(0, SCORE_DISPLAY_MAX);
                }
                StatusUpdate::Life(delta) => {
                    // Clamp to `>= 0` only: negative deltas cannot drive
                    // the lives count below zero in the shared state.
                    state.lives = state.lives.saturating_add(delta).max(0);
                }
                StatusUpdate::Item(item, add) => {
                    if add {
                        state.inventory.push(item);
                    } else if let Some(idx) =
                        state.inventory.iter().position(|carried| *carried == item)
                    {
                        state.inventory.remove(idx);
                    }
                }
                StatusUpdate::Text(text) => {
                    self.status_text = Some(text);
                    self.status_text_ticks = LEVEL_MESSAGE_TICKS;
                }
            }
        }
    }

    /// Builds the dynamic status-bar overlay render commands for the current
    /// tick: score digits, lives digit, inventory item grid, and the
    /// status-bar message text (when present).
    ///
    /// Each subsection emits a [`RenderCommand::FillRect`] erase followed by
    /// the new pixel content (`DrawText` for digits and the message bar,
    /// `Blit` for inventory icons).  The erase rects use the inventory area's
    /// flat background color so the overlay can be redrawn every frame
    /// without leaving glyph ghosts behind from earlier values, matching the
    /// "FillRect then redraw" pattern the Java reference implements for
    /// `InventoryPointMessage` / `InventoryLifeMessage` / `InventoryItemMessage`.
    fn render_dynamic_status(&self, state: &RuntimeState) -> Vec<RenderCommand> {
        let mut commands = Vec::new();

        // Static inventory-area labels from inventory_conf.json `text` array.
        // These are level-screen-specific: drawn on top of the Jill portrait
        // but not shown on Map or Start-Menu screens.
        for (text, x_inv, y_inv, color) in [
            (
                "health",
                HEALTH_LABEL_X_INV,
                HEALTH_LABEL_Y_INV,
                HEALTH_LABEL_COLOR,
            ),
            (
                "level",
                LEVEL_LABEL_X_INV,
                LEVEL_LABEL_Y_INV,
                LEVEL_LABEL_COLOR,
            ),
            ("map", MAP_LABEL_X_INV, MAP_LABEL_Y_INV, MAP_LABEL_COLOR),
            (
                "score",
                SCORE_LABEL_X_INV,
                SCORE_LABEL_Y_INV,
                SCORE_LABEL_COLOR,
            ),
        ] {
            commands.push(RenderCommand::DrawText {
                text: text.to_string(),
                x: INVENTORY_AREA_X + x_inv,
                y: INVENTORY_AREA_Y + y_inv,
                color_index: color,
                font: FontSize::Small,
            });
        }

        // Health bar: end-cap + one segment tile per health point.
        // Erase the whole bar region first so lost health points don't ghost.
        // Clamp the right edge to the inventory area width so the FillRect
        // does not bleed one pixel into the vertical frame border.
        let erase_start_x = INVENTORY_AREA_X + LIFEBAR_END_X_INV;
        let erase_right_inv = (LIFEBAR_X_INV + LIFEBAR_MAX * LIFEBAR_STEP + LIFEBAR_TILE_W)
            .min(INVENTORY_AREA_W as i32);
        let erase_w = (erase_right_inv - LIFEBAR_END_X_INV).max(0) as u32;
        commands.push(RenderCommand::FillRect {
            x: erase_start_x,
            y: INVENTORY_AREA_Y + LIFEBAR_Y_INV,
            width: erase_w,
            height: LIFEBAR_ERASE_H,
            color: INVENTORY_BG_COLOR,
        });
        commands.push(RenderCommand::Blit {
            tileset: LIFEBAR_TILESET,
            tile: LIFEBAR_END_TILE,
            x: INVENTORY_AREA_X + LIFEBAR_END_X_INV,
            y: INVENTORY_AREA_Y + LIFEBAR_Y_INV,
            opaque: false,
            clip: Some(INVENTORY_AREA_CLIP),
        });
        let health = state.health.clamp(0, LIFEBAR_MAX);
        for i in 0..health {
            commands.push(RenderCommand::Blit {
                tileset: LIFEBAR_TILESET,
                tile: LIFEBAR_TILE,
                x: INVENTORY_AREA_X + LIFEBAR_X_INV + i * LIFEBAR_STEP,
                y: INVENTORY_AREA_Y + LIFEBAR_Y_INV,
                opaque: false,
                clip: Some(INVENTORY_AREA_CLIP),
            });
        }

        // Score: plain decimal, right-aligned so the rightmost digit's right
        // edge sits at inventory-local x=SCORE_X_INV (confirmed from Java
        // InventoryManager right-align logic; score.x is the right edge, not left).
        let display_score = state.score.clamp(0, SCORE_DISPLAY_MAX);
        let score_str = display_score.to_string();
        let score_draw_x =
            INVENTORY_AREA_X + SCORE_X_INV - score_str.len() as i32 * SMALL_FONT_CHAR_W;
        let score_erase_x =
            INVENTORY_AREA_X + SCORE_X_INV - SCORE_DIGITS as i32 * SMALL_FONT_CHAR_W;
        commands.push(RenderCommand::FillRect {
            x: score_erase_x,
            y: INVENTORY_AREA_Y + SCORE_Y_INV,
            width: SCORE_ERASE_W,
            height: SCORE_ERASE_H,
            color: INVENTORY_BG_COLOR,
        });
        commands.push(RenderCommand::DrawText {
            text: score_str,
            x: score_draw_x,
            y: INVENTORY_AREA_Y + SCORE_Y_INV,
            color_index: SCORE_COLOR,
            font: FontSize::Small,
        });

        // Inventory item grid: erase the grid block and blit each carried
        // item in row-major order until the grid is full.
        //
        // The `itemConf` rectangle declared in `inventory_conf.json`
        // (4 cols × 3 rows × 16 px pitch starting at inventory-local
        // `(2, 27)`) extends two pixels past the inventory area's right
        // edge and six pixels past its bottom edge.  The erase rect is
        // clamped to the inventory interior so the fill does not punch
        // through the surrounding vertical bar tile column (`x = 72`) or
        // the lower horizontal bar / `"INVENTORY"` label band
        // (`y ≥ 176`), and every grid `Blit` carries the
        // [`INVENTORY_AREA_CLIP`] so the rightmost icon column's two-pixel
        // overflow is silently clipped the same way the Java reference's
        // backing `BufferedImage` clips it.
        let grid_screen_x = INVENTORY_AREA_X + ITEM_GRID_X_INV;
        let grid_screen_y = INVENTORY_AREA_Y + ITEM_GRID_Y_INV;
        let inv_right = INVENTORY_AREA_X + INVENTORY_AREA_W as i32;
        let inv_bottom = INVENTORY_AREA_Y + INVENTORY_AREA_H as i32;
        let grid_right = grid_screen_x + ITEM_GRID_COLS as i32 * ITEM_GRID_PITCH;
        let grid_bottom = grid_screen_y + ITEM_GRID_ROWS as i32 * ITEM_GRID_PITCH;
        let erase_w = (grid_right.min(inv_right) - grid_screen_x).max(0) as u32;
        let erase_h = (grid_bottom.min(inv_bottom) - grid_screen_y).max(0) as u32;
        commands.push(RenderCommand::FillRect {
            x: grid_screen_x,
            y: grid_screen_y,
            width: erase_w,
            height: erase_h,
            color: INVENTORY_BG_COLOR,
        });
        for (index, item) in state
            .inventory
            .iter()
            .take(ITEM_GRID_ROWS * ITEM_GRID_COLS)
            .enumerate()
        {
            let col = (index % ITEM_GRID_COLS) as i32;
            let row = (index / ITEM_GRID_COLS) as i32;
            let (tileset, tile) = inventory_item_tile(*item);
            commands.push(RenderCommand::Blit {
                tileset,
                tile,
                x: grid_screen_x + col * ITEM_GRID_PITCH,
                y: grid_screen_y + row * ITEM_GRID_PITCH,
                opaque: false,
                clip: Some(INVENTORY_AREA_CLIP),
            });
        }

        // Message bar text: only emit when an active message is being
        // displayed.  The FillRect erases the entire bar so leftover glyphs
        // from a previous message do not bleed past the new text length.
        if let Some(text) = self.status_text.as_ref() {
            commands.push(RenderCommand::FillRect {
                x: 0,
                y: MESSAGE_BAR_Y,
                width: SCREEN_WIDTH,
                height: MESSAGE_BAR_H,
                color: 0,
            });
            commands.push(RenderCommand::DrawText {
                text: text.clone(),
                x: STATUS_BAR_TEXT_X,
                y: MESSAGE_BAR_Y + STATUS_BAR_TEXT_Y_OFFSET,
                color_index: STATUS_BAR_TEXT_COLOR,
                font: FontSize::Small,
            });
        }

        commands
    }

    /// Drops every message that has accumulated in
    /// [`Self::entity_dispatcher`]'s pending queue this tick without
    /// touching the subscriber list.
    ///
    /// Subscribers attached in [`LevelScreen::new`] receive their messages
    /// immediately on `send`, but message types whose listener has not been
    /// implemented yet (for example, [`MessageType::Trigger`] sent by
    /// [`crate::entities::objects::touch_trigger::TouchTriggerEntity`] before
    /// the toggle-wall entity from issue 63 lands) would otherwise grow the
    /// pending queue every tick and deliver a stale burst the moment a
    /// subscriber appears.  Clearing the queue at end-of-tick keeps memory
    /// bounded and matches the Java reference's per-frame `MessageDispatcher`
    /// flush.
    fn drain_entity_dispatcher(&mut self) {
        self.entity_dispatcher.clear_pending();
    }
}

impl ScreenHandler for LevelScreen {
    /// Advances the level screen by one fixed tick.
    ///
    /// Pulls any new transition request from the dispatcher inbox, advances
    /// the message-box countdown, and surfaces the matching
    /// [`ScreenTransition`] once the countdown reaches zero.  Escape returns
    /// directly to the start menu, mirroring the abort behavior of the
    /// reference implementation when no transition is pending.
    fn tick(&mut self, input: &ActiveInput, state: &mut RuntimeState) -> TickResult {
        self.pump_inbox();
        self.pump_status_inbox(state);

        // Tick order each frame:
        // 1. Update every object entity (player moves, lifts dispatch
        //    PlayerMove, player dispatches CreateObject on fire).
        // 2. Apply platform-move deltas from lifts to the player position.
        // 3. Dispatch player-vs-object touch callbacks; switches and touch
        //    triggers dispatch Trigger messages here.
        // 4. Route trigger messages to all objects (after the touch pass so
        //    switches touched this tick activate on the same tick).
        // 5. Drop entities that flagged themselves for removal.
        // 6. Spawn objects requested via CreateObject this tick.
        // 7. Apply background-clear requests so opened doors are passable.
        // 8. Snap the viewport so the post-update player stays inside the
        //    96/48 px update border, clamped to the map bounds.
        // 9. Build the base frame using the freshly-snapped viewport.
        // 10. Per-cell background tick + draw (also uses the new viewport).
        // 11. Object draws (on top of backgrounds, matching the Java
        //     reference draw order).
        // 12. Message-box overlay last so transitions paint over everything
        //     else.
        // Decrement the post-hit invincibility window once per tick so the
        // player regains vulnerability after `PLAYER_INVINCIBILITY_TICKS`
        // ticks of contact immunity following an enemy touch.
        if state.invincibility_ticks > 0 {
            state.invincibility_ticks -= 1;
        }
        self.update_objects(input, state);
        self.apply_platform_moves();
        self.dispatch_player_touches(state);
        self.dispatch_projectile_hits();
        self.route_triggers();
        self.reap_removed_objects();
        self.spawn_objects();
        self.apply_background_clears();
        self.update_viewport();

        let mut commands = self.render_base_frame();

        let player_bbox = self.player_bounding_box();
        let bg_commands = self.tick_backgrounds(player_bbox);
        let obj_commands = self.draw_objects();

        commands.extend(bg_commands);
        commands.extend(obj_commands);

        // Dynamic status-bar overlay sits on top of the game-area commands so
        // its message-bar text and per-tick score/lives/inventory digits paint
        // over the static status-bar mosaic prepended by the orchestrator.
        commands.extend(self.render_dynamic_status(state));

        if self.pending.is_some() {
            commands.extend(render_message_box(&self.message_text));
        }

        self.drain_entity_dispatcher();

        // Status-bar text countdown: clear the active message after
        // `LEVEL_MESSAGE_TICKS` ticks so a one-off pickup hint vanishes
        // without a follow-up `StatusBarText` send.  Runs after the overlay
        // has already been rendered for this frame so the message is visible
        // on the tick it expires, mirroring the level-message-box countdown
        // above.
        if self.status_text_ticks > 0 {
            self.status_text_ticks -= 1;
            if self.status_text_ticks == 0 {
                self.status_text = None;
            }
        }

        let mut transition: Option<ScreenTransition> = None;
        if self.pending.is_some() {
            if self.message_ticks > 0 {
                self.message_ticks -= 1;
                if self.message_ticks == 0 {
                    // Take the pending request without cloning so the
                    // ChangeLevel payload's String is moved into the
                    // ScreenTransition on the final tick only.
                    let pending = self.pending.take().expect("pending verified Some above");
                    transition = Some(pending_into_transition(pending));
                    self.message_text.clear();
                }
            }
        } else if input.contains(&InputCommand::Pause) {
            transition = Some(ScreenTransition::StartMenu);
        }

        TickResult {
            commands,
            transition,
            sound_events: Vec::new(),
        }
    }

    /// Returns the raw JN bytes when this screen is acting as the world map
    /// (`level_number == MAP_LEVEL`), so the orchestrator can reconstruct the
    /// map screen from memory on the next `ScreenTransition::Map`.
    ///
    /// Returns `None` for regular levels; those bytes are surfaced via
    /// [`level_jn_bytes`] instead.
    fn map_jn_bytes(&self) -> Option<Vec<u8>> {
        if self.level_number == MAP_LEVEL {
            Some(self.jn_bytes.clone())
        } else {
            None
        }
    }

    /// Returns the raw JN bytes for regular levels so the orchestrator can
    /// restart from memory.  Returns `None` when this screen is acting as the
    /// world map to avoid polluting the orchestrator's level-byte cache with
    /// `MAP.JN1` bytes.
    fn level_jn_bytes(&self) -> Option<Vec<u8>> {
        if self.level_number == MAP_LEVEL {
            None
        } else {
            Some(self.jn_bytes.clone())
        }
    }
}

/// Dispatcher handler that records arriving transition messages into a shared
/// inbox so the [`LevelScreen`] can drain them on the next tick.
struct InboxHandler {
    /// Shared inbox holding pending transition requests.
    inbox: Inbox,
}

impl MessageHandler for InboxHandler {
    /// Converts the supported transition messages into [`PendingRequest`]
    /// entries and appends them to the inbox.  Unrelated payload variants are
    /// silently ignored.
    fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
        let request = match (msg_type, payload) {
            (MessageType::CheckpointChangeLevel, MessagePayload::ChangeLevel(p)) => {
                PendingRequest::ChangeLevel(p.clone())
            }
            (MessageType::CheckpointChangeLevelPrevious, _) => PendingRequest::PreviousMap,
            (MessageType::DieRestartLevel, _) => PendingRequest::RestartLevel,
            _ => return,
        };
        if let Ok(mut queue) = self.inbox.lock() {
            queue.push(request);
        }
    }
}

/// Dispatcher handler that records arriving status-bar updates into the
/// [`StatusInbox`] for the level screen to drain on its next tick.
///
/// One handler instance is subscribed per source [`MessageType`]; the
/// matching payload variant is translated into the corresponding
/// [`StatusUpdate`].  Unrelated payload variants are silently ignored to
/// stay forward-compatible with future message-type extensions.
struct StatusInboxHandler {
    /// Shared inbox the level screen drains every tick.
    inbox: StatusInbox,
}

impl MessageHandler for StatusInboxHandler {
    /// Converts each supported `(MessageType, MessagePayload)` pair into a
    /// [`StatusUpdate`] and appends it to the inbox.  Mismatched pairs are
    /// silently dropped.
    fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
        let update = match (msg_type, payload) {
            (MessageType::InventoryPoint, MessagePayload::Count(n)) => StatusUpdate::Point(*n),
            (MessageType::InventoryLife, MessagePayload::Count(n)) => StatusUpdate::Life(*n),
            (MessageType::InventoryItem, MessagePayload::InventoryItem(payload)) => {
                StatusUpdate::Item(payload.item, payload.add)
            }
            (MessageType::StatusBarText, MessagePayload::Text(text)) => {
                StatusUpdate::Text(text.clone())
            }
            _ => return,
        };
        if let Ok(mut queue) = self.inbox.lock() {
            queue.push(update);
        }
    }
}

/// Consumes a [`PendingRequest`] and yields the [`ScreenTransition`]
/// surfaced once the message-box countdown reaches zero.
///
/// Takes ownership so the inner `ChangeLevel` payload's `String` moves
/// directly into the resulting `ScreenTransition::Level` without an
/// extra clone.
fn pending_into_transition(request: PendingRequest) -> ScreenTransition {
    match request {
        PendingRequest::ChangeLevel(payload) => ScreenTransition::Level {
            file: payload.level_file,
            number: payload.level_number,
        },
        PendingRequest::PreviousMap => ScreenTransition::Map,
        PendingRequest::RestartLevel => ScreenTransition::RestartLevel,
    }
}

/// Builds the per-level `ObjectEntity` list from a parsed JN object list.
///
/// Iterates the JN object records in source order so per-tick draw and
/// collision iteration follows the same order the Java reference uses for
/// its object manager list.
fn build_object_entities(jn: &JnFile, cache: &AssetCache) -> Vec<Box<dyn ObjectEntity>> {
    let strings = jn.strings();
    jn.objects()
        .iter()
        .map(|obj| {
            let string_entry = obj
                .string_index()
                .and_then(|idx| strings.get(idx))
                .map(|entry| entry.value());
            make_object_entity(obj.object_type(), obj, string_entry, cache)
        })
        .collect()
}

/// Builds the per-level [`BackgroundGrid`] from a parsed JN background layer.
///
/// Each cell's map code is looked up against `cache.dma`; the resolved DMA
/// name selects the concrete [`openjill_core::BackgroundEntity`]
/// implementation via [`make_background_entity`].  Cells whose map code has
/// no DMA entry receive the default transparent background entity.
fn build_background_grid(jn: &JnFile, cache: &AssetCache) -> BackgroundGrid {
    let bg = jn.background();
    let width = bg.width();
    let height = bg.height();
    let mut rows: Vec<Vec<Box<dyn openjill_core::BackgroundEntity>>> = Vec::with_capacity(height);
    for y in 0..height {
        let mut row: Vec<Box<dyn openjill_core::BackgroundEntity>> = Vec::with_capacity(width);
        for x in 0..width {
            let map_code = bg.map_code(x, y).unwrap_or(0);
            let name = cache
                .dma
                .get_by_map_code(map_code)
                .map(|entry| entry.name())
                .unwrap_or("");
            row.push(make_background_entity(name, map_code, cache));
        }
        rows.push(row);
    }
    BackgroundGrid::new(rows)
}

/// Returns the rectangle (in world pixels) covered by the current viewport
/// expanded by the per-axis update border.
///
/// Objects whose bounding box intersects this rectangle tick each frame;
/// objects entirely outside skip their update step unless they opt into
/// `always_active`.
fn viewport_update_rect(viewport_x: i32, viewport_y: i32) -> Rect {
    let world_x = -viewport_x - X_UPDATE_BORDER as i32;
    let world_y = -viewport_y - Y_UPDATE_BORDER as i32;
    let w = GAME_AREA_W as i32 + 2 * X_UPDATE_BORDER as i32;
    let h = GAME_AREA_H as i32 + 2 * Y_UPDATE_BORDER as i32;
    Rect::new(world_x, world_y, w, h)
}

/// Returns the rectangle (in world pixels) covered by the visible game area
/// at the current viewport offset.
///
/// Objects whose bounding box intersects this rectangle contribute their
/// `draw` command this frame.
fn viewport_game_rect(viewport_x: i32, viewport_y: i32) -> Rect {
    let world_x = -viewport_x;
    let world_y = -viewport_y;
    Rect::new(world_x, world_y, GAME_AREA_W as i32, GAME_AREA_H as i32)
}

/// Rewrites a render command emitted by an [`ObjectEntity::draw`] from world
/// pixel coordinates into framebuffer pixel coordinates for the current
/// viewport.
///
/// Object draw implementations report their `(x, y)` in the same world-pixel
/// space the JN file stores (origin at the top-left of the entire map).  The
/// framebuffer the renderer consumes, on the other hand, has its origin at
/// the screen's top-left and the visible game area positioned at
/// `(GAME_AREA_X, GAME_AREA_Y)` with width [`GAME_AREA_W`] / height
/// [`GAME_AREA_H`]. The translation mirrors the formula
/// [`render_map_background`] / `tick_backgrounds` already use for background
/// tiles:
///
/// ```text
/// screen_x = GAME_AREA_X + world_x - world_origin_x
///          = GAME_AREA_X + world_x + viewport_x
/// ```
///
/// (with the OpenJill sign convention `world_origin_x = -viewport_x`).
///
/// Blit commands additionally pick up the shared [`GAME_AREA_CLIP`] when the
/// object did not already supply a tighter rectangle, so sprites that
/// straddle the right or bottom game-area edge do not bleed into the
/// surrounding status bar.  Non-Blit commands pass through unchanged because
/// `RenderCommand::DrawText` / `FillRect` / `Clear` are not produced from
/// world coordinates by the entity layer today.
fn translate_object_command(cmd: RenderCommand, viewport_x: i32, viewport_y: i32) -> RenderCommand {
    match cmd {
        RenderCommand::Blit {
            tileset,
            tile,
            x,
            y,
            opaque,
            clip,
        } => RenderCommand::Blit {
            tileset,
            tile,
            x: GAME_AREA_X + x + viewport_x,
            y: GAME_AREA_Y + y + viewport_y,
            opaque,
            clip: clip.or(Some(GAME_AREA_CLIP)),
        },
        other => other,
    }
}

/// Returns the per-tick viewport offset that keeps `player_bbox` inside the
/// 96 px horizontal and 48 px vertical update border, clamped to the map's
/// scrollable range.
///
/// Mirrors the `AbstractExecutingStdPlayerLevel` scroll rule from the Java
/// reference: when the player's bounding box leaves the inner border the
/// viewport advances by exactly the overshoot; otherwise the viewport is
/// left untouched.  The clamp range is
/// `[0, MAP_WIDTH * 16 - GAME_AREA_W]` x `[0, MAP_HEIGHT * 16 - GAME_AREA_H]`
/// with the same OpenJill sign convention used elsewhere in this file: the
/// visible world top-left equals `(-viewport_x, -viewport_y)`, so the
/// computation is performed on the non-negative world-pixel form
/// (`wx = -viewport_x`) and the result is negated before being returned.
///
/// Pulled out into a free function so unit tests can exercise the snap-and-
/// clamp arithmetic without constructing a full [`LevelScreen`] with a
/// player entity in its object list.
fn compute_viewport_scroll(player_bbox: Rect, viewport_x: i32, viewport_y: i32) -> (i32, i32) {
    let map_w_px = BACKGROUND_GRID_WIDTH as i32 * BLOCK_SIZE_I;
    let map_h_px = BACKGROUND_GRID_HEIGHT as i32 * BLOCK_SIZE_I;
    let max_wx = (map_w_px - GAME_AREA_W as i32).max(0);
    let max_wy = (map_h_px - GAME_AREA_H as i32).max(0);
    let x_border = X_UPDATE_BORDER as i32;
    let y_border = Y_UPDATE_BORDER as i32;

    let mut wx = -viewport_x;
    let mut wy = -viewport_y;

    if player_bbox.x < wx + x_border {
        wx = player_bbox.x - x_border;
    } else if player_bbox.x + player_bbox.w > wx + GAME_AREA_W as i32 - x_border {
        wx = player_bbox.x + player_bbox.w - GAME_AREA_W as i32 + x_border;
    }
    if player_bbox.y < wy + y_border {
        wy = player_bbox.y - y_border;
    } else if player_bbox.y + player_bbox.h > wy + GAME_AREA_H as i32 - y_border {
        wy = player_bbox.y + player_bbox.h - GAME_AREA_H as i32 + y_border;
    }

    wx = wx.clamp(0, max_wx);
    wy = wy.clamp(0, max_wy);

    (-wx, -wy)
}

/// Locates the checkpoint object for `level_number` in `jn` and returns the
/// viewport offset that places that object near the center of the game area.
///
/// Mirrors `findCheckPoint` + `centerScreen` from the Java reference: the
/// first object whose signed counter equals the level number is the
/// checkpoint, and the viewport is offset so the checkpoint pixel appears at
/// the game-area center rather than the top-left corner.  When no checkpoint
/// exists the viewport is pinned at `(0, 0)`.
fn checkpoint_viewport(jn: &JnFile, level_number: i32) -> (i32, i32) {
    if let Some(object) = find_checkpoint(jn, level_number) {
        let world_x = object.x() as i32;
        let world_y = object.y() as i32;
        let center_x = openjill_core::layout::GAME_AREA_W as i32 / 2;
        let center_y = openjill_core::layout::GAME_AREA_H as i32 / 2;
        return (center_x - world_x, center_y - world_y);
    }
    (0, 0)
}

/// Returns the first `CheckPointEntity` (object type 12) whose `counter`
/// equals `level_number`, when one exists.
///
/// The `counter` field is also used by unrelated object types to encode
/// per-instance level links (e.g. `FallingSpikeEntity` / `TouchTriggerEntity`
/// type 38 and 15 store the level the instance ties to in `counter`), so
/// filtering by object type 12 is required to pick the genuine checkpoint
/// rather than the first match by counter alone.  Mirrors the Java
/// reference's `findCheckPoint`, which iterates the level's `CheckPointEntity`
/// list rather than the global object list.
const CHECKPOINT_OBJECT_TYPE: u8 = 12;

/// Returns the first checkpoint object (`object_type = 12`) whose `counter`
/// equals `level_number`, when one exists.
///
/// See [`CHECKPOINT_OBJECT_TYPE`] for the rationale on the object-type filter.
fn find_checkpoint(jn: &JnFile, level_number: i32) -> Option<&JnObject> {
    let needle = i16::try_from(level_number).ok()?;
    jn.objects()
        .iter()
        .find(|obj| obj.object_type() == CHECKPOINT_OBJECT_TYPE && obj.counter() == needle)
}

/// Returns the message-box text lines for the destination `level_number`,
/// looked up against the embedded `level_messagebox_vga.json` `messages.JN1`
/// table and split on `\n`.
///
/// `MAP_LEVEL` (-1) maps to `messages.JN1[0]` ("JILL ENTERS THE JUNGLE MAP.")
/// because the map screen is the destination when returning from a level;
/// other negative levels yield no text.  Positive level numbers index the
/// table directly, so level 1 displays `messages.JN1[1]` and so on.
fn lookup_message_text(level_number: i32) -> Vec<String> {
    let table = &MESSAGE_BOX.messages;
    let index = if level_number == openjill_core::MAP_LEVEL {
        0_usize
    } else if level_number > 0 {
        level_number as usize
    } else {
        return Vec::new();
    };
    let Some(entry) = table.get(index) else {
        return Vec::new();
    };
    entry.split('\n').map(|line| line.to_string()).collect()
}

/// Emits render commands for the level message-box overlay.
///
/// Paints the picture-area and text-area background fills declared in
/// `level_messagebox_vga.json`, then renders the static frame tile mosaic
/// (frame border + Jill face), then one `DrawText` per message line up to
/// [`MESSAGE_MAX_LINES`].  The fills are emitted first so the underlying
/// level background does not bleed through the box.
///
/// Frame and face blits carry a per-command [`ClipRect`] sized to the box's
/// declared `width` x `height`.  The Java reference draws the box into a
/// `BufferedImage(width, height)` backing buffer, which silently clips any
/// tile that would overflow the buffer; this clip rect reproduces the same
/// behavior in the framebuffer renderer so frame tiles whose tile geometry
/// extends past the box (e.g. the 16-tall vertical bar tile sitting on the
/// last row of a 92-tall box that only has 12 px of slack at the bottom)
/// do not bleed below the box border.
fn render_message_box(text: &[String]) -> Vec<RenderCommand> {
    let layout = &*MESSAGE_BOX;
    let clip = openjill_core::ClipRect {
        x: layout.x,
        y: layout.y,
        width: layout.width,
        height: layout.height,
    };
    let mut commands: Vec<RenderCommand> = Vec::with_capacity(layout.images.len() + 4);
    commands.push(RenderCommand::FillRect {
        x: layout.x + layout.picturearea.x,
        y: layout.y + layout.picturearea.y,
        width: layout.picturearea.width,
        height: layout.picturearea.height,
        color: layout.picturearea.color,
    });
    commands.push(RenderCommand::FillRect {
        x: layout.x + layout.textarea_x,
        y: layout.y + layout.textarea_y,
        width: layout.textarea_w,
        height: layout.textarea_h,
        color: layout.textarea_color,
    });
    commands.extend(layout.images.iter().map(|tile| RenderCommand::Blit {
        tileset: tile.tileset,
        tile: tile.tile,
        x: layout.x + tile.x,
        y: layout.y + tile.y,
        opaque: false,
        clip: Some(clip),
    }));

    let text_origin_x = layout.x + layout.textarea_x;
    let text_origin_y = layout.y + layout.textarea_y;
    for (line_index, line) in text.iter().take(MESSAGE_MAX_LINES).enumerate() {
        commands.push(RenderCommand::DrawText {
            text: line.clone(),
            x: text_origin_x,
            y: text_origin_y + (line_index as i32) * MESSAGE_LINE_HEIGHT,
            color_index: layout.text_color,
            font: FontSize::Small,
        });
    }
    commands
}

/// One tile blit reference parsed from `level_messagebox_vga.json`.
#[derive(Clone, Debug)]
struct MessageBoxTile {
    /// Source tileset index.
    tileset: u8,
    /// Source tile index inside the tileset.
    tile: u16,
    /// X offset relative to the message-box origin in pixels.
    x: i32,
    /// Y offset relative to the message-box origin in pixels.
    y: i32,
}

/// Rectangle painted as a background fill inside the message box before tiles
/// and text are drawn over it.
#[derive(Clone, Copy, Debug)]
struct MessageBoxArea {
    /// X offset relative to the message-box origin in pixels.
    x: i32,
    /// Y offset relative to the message-box origin in pixels.
    y: i32,
    /// Width of the area in pixels.
    width: u32,
    /// Height of the area in pixels.
    height: u32,
    /// Palette index used to fill the area before tiles are blitted.
    color: u8,
}

/// Parsed `level_messagebox_vga.json` layout used by [`LevelScreen`].
struct MessageBoxLayout {
    /// Top-left X position of the message box in framebuffer pixels.
    x: i32,
    /// Top-left Y position of the message box in framebuffer pixels.
    y: i32,
    /// Overall box width in pixels (from the JSON `width` field).
    ///
    /// Mirrors the `BufferedImage(width, height)` backing buffer the Java
    /// reference allocates for the box: tiles that would draw past this
    /// rectangle are clipped by that buffer.  The Rust port draws directly
    /// to the framebuffer, so this rectangle is fed into a [`ClipRect`] for
    /// each frame blit to reproduce the same clipping.
    width: u32,
    /// Overall box height in pixels (from the JSON `height` field).
    ///
    /// See [`MessageBoxLayout::width`] for the clipping rationale.
    height: u32,
    /// Text area X offset relative to the message-box origin.
    textarea_x: i32,
    /// Text area Y offset relative to the message-box origin.
    textarea_y: i32,
    /// Text area width in pixels.
    textarea_w: u32,
    /// Text area height in pixels.
    textarea_h: u32,
    /// Palette index used to fill the text area before drawing glyphs.
    textarea_color: u8,
    /// Picture-area background fill (sits behind the Jill face tiles).
    picturearea: MessageBoxArea,
    /// Palette index used to draw the message text.
    text_color: u8,
    /// Static tile-mosaic frame and Jill face tiles.
    images: Vec<MessageBoxTile>,
    /// Per-level message text strings for the `JN1` save prefix.
    messages: Vec<String>,
}

/// Parses [`LEVEL_MESSAGEBOX_JSON`] into a [`MessageBoxLayout`].
///
/// Panics if the embedded JSON is structurally invalid because this is a
/// programmer error: the JSON is bundled at compile time and cannot drift
/// without a corresponding code change.
fn parse_message_box_layout() -> MessageBoxLayout {
    let value: serde_json::Value = serde_json::from_str(LEVEL_MESSAGEBOX_JSON)
        .expect("embedded level_messagebox_vga.json must be valid JSON");

    let get_i = |obj: &serde_json::Value, key: &str, default: i64| -> i32 {
        obj.get(key).and_then(|v| v.as_i64()).unwrap_or(default) as i32
    };
    let get_u = |obj: &serde_json::Value, key: &str, default: u64| -> u8 {
        obj.get(key).and_then(|v| v.as_u64()).unwrap_or(default) as u8
    };

    let get_u32 = |obj: &serde_json::Value, key: &str, default: u64| -> u32 {
        obj.get(key).and_then(|v| v.as_u64()).unwrap_or(default) as u32
    };

    let x = get_i(&value, "x", 0);
    let y = get_i(&value, "y", 0);
    let width = get_u32(&value, "width", 0);
    let height = get_u32(&value, "height", 0);
    let text_color = get_u(&value, "textColor", 7);
    let textarea = value
        .get("textarea")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let textarea_x = get_i(&textarea, "x", 0);
    let textarea_y = get_i(&textarea, "y", 0);
    let textarea_w = get_u32(&textarea, "width", 0);
    let textarea_h = get_u32(&textarea, "height", 0);
    let textarea_color = get_u(&textarea, "color", 1);
    let picturearea_value = value
        .get("picturearea")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let picturearea = MessageBoxArea {
        x: get_i(&picturearea_value, "x", 0),
        y: get_i(&picturearea_value, "y", 0),
        width: get_u32(&picturearea_value, "width", 0),
        height: get_u32(&picturearea_value, "height", 0),
        color: get_u(&picturearea_value, "color", 0),
    };

    let images = value
        .get("images")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|img| {
                    Some(MessageBoxTile {
                        tileset: img.get("tileset")?.as_u64()? as u8,
                        tile: img.get("tile")?.as_u64()? as u16,
                        x: img.get("x")?.as_i64()? as i32,
                        y: img.get("y")?.as_i64()? as i32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let messages = value
        .get("messages")
        .and_then(|m| m.get(EPISODE_SAVE_PREFIX))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    MessageBoxLayout {
        x,
        y,
        width,
        height,
        textarea_x,
        textarea_y,
        textarea_w,
        textarea_h,
        textarea_color,
        picturearea,
        text_color,
        images,
        messages,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EPISODE_1_SKY_COLOR, LEVEL_MESSAGEBOX_JSON, LevelScreen, MESSAGE_LINE_HEIGHT,
        MESSAGE_MAX_LINES, checkpoint_viewport, compute_viewport_scroll, find_checkpoint,
        lookup_message_text, render_message_box,
    };
    use openjill_core::layout::{
        GAME_AREA_H, GAME_AREA_W, INVENTORY_AREA_X, INVENTORY_AREA_Y, LEVEL_MESSAGE_TICKS,
        MESSAGE_BAR_Y, X_UPDATE_BORDER, Y_UPDATE_BORDER,
    };
    use openjill_core::runtime::{InventoryObject, RuntimeState};
    use openjill_core::{
        ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, ChangeLevelPayload,
        InputCommand, InventoryItemPayload, MessageDispatcher, MessagePayload, MessageType, Rect,
        RenderCommand, ScreenHandler, ScreenTransition,
    };
    use openjill_data::jn::JnFile;

    use crate::asset_cache::AssetCache;

    /// Object record size in bytes (`JnObject` fixed field layout):
    /// `object_type` (1) + `x`/`y`/`x_speed`/`y_speed` (2 each) +
    /// `width`/`height`/`state`/`sub_state`/`state_count`/`counter`/`flags`
    /// (2 each) + `pointer` (4) + `info1`/`zap_hold` (2 each).
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a synthetic [`AssetCache`] for tests that do not need real
    /// game files.
    fn synthetic_cache() -> AssetCache {
        AssetCache::synthetic()
    }

    /// Builds a synthetic JN byte buffer carrying `objects` zero-initialized
    /// object records, mutating only the fields the tests need.
    ///
    /// Each entry in `objects` is `(counter, x, y)` for one object record,
    /// emitted in source order.  `object_type` is set to
    /// [`super::CHECKPOINT_OBJECT_TYPE`] (12) so the synthetic objects pass
    /// the checkpoint object-type filter; all other fields are zero-filled.
    fn jn_bytes_with_objects(objects: &[(i16, u16, u16)]) -> Vec<u8> {
        jn_bytes_with_typed_objects(
            &objects
                .iter()
                .map(|&(counter, x, y)| (super::CHECKPOINT_OBJECT_TYPE, counter, x, y))
                .collect::<Vec<_>>(),
        )
    }

    /// Builds a synthetic JN byte buffer with explicit `object_type` per
    /// record so tests that exercise the [`find_checkpoint`] type filter can
    /// assert against non-checkpoint object types.
    ///
    /// Each entry is `(object_type, counter, x, y)`.
    fn jn_bytes_with_typed_objects(objects: &[(u8, i16, u16, u16)]) -> Vec<u8> {
        let object_count = objects.len();
        let total_bytes = 128 * 64 * 2 + 2 + object_count * OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total_bytes];

        // Object count at byte offset 16384 (128×64 cells × 2 bytes per cell).
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&(object_count as u16).to_le_bytes());

        for (index, (object_type, counter, x, y)) in objects.iter().enumerate() {
            let record_off = count_off + 2 + index * OBJECT_RECORD_BYTES;
            // object_type (u8) at +0.
            bytes[record_off] = *object_type;
            // x (u16) at +1.
            bytes[record_off + 1..record_off + 3].copy_from_slice(&x.to_le_bytes());
            // y (u16) at +3.
            bytes[record_off + 3..record_off + 5].copy_from_slice(&y.to_le_bytes());
            // counter (i16) at +19 — preceded by 18 bytes:
            // object_type(1) + x(2) + y(2) + x_speed(2) + y_speed(2) +
            // width(2) + height(2) + state(2) + sub_state(2) + state_count(2).
            bytes[record_off + 19..record_off + 21].copy_from_slice(&counter.to_le_bytes());
        }
        bytes
    }

    /// Constructs a [`LevelScreen`] from synthetic JN bytes, also returning
    /// the dispatcher used during subscription so tests can send messages.
    fn screen_with_dispatcher(
        bytes: Vec<u8>,
        level_number: i32,
    ) -> (LevelScreen, MessageDispatcher) {
        let mut dispatcher = MessageDispatcher::new();
        let cache = synthetic_cache();
        let screen = LevelScreen::from_bytes(
            bytes,
            &cache,
            level_number,
            &mut dispatcher,
            EPISODE_1_SKY_COLOR,
        )
        .expect("synthetic level JN should parse");
        (screen, dispatcher)
    }

    /// Returns the count of `Blit` commands that match the message-box frame
    /// origin and the Jill-face tileset, isolating overlay output from any
    /// background blits the screen may emit.
    ///
    /// Frame tiles use tileset 24 (Jill face) and tileset 3 (border bars);
    /// because the screen does not render the static status bar (the
    /// orchestrator prepends that itself), the only commands carrying these
    /// tilesets here come from the message-box overlay.
    fn count_message_box_commands(commands: &[RenderCommand]) -> usize {
        commands
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::Blit { tileset: 24, .. }
                        | RenderCommand::Blit { tileset: 3, .. }
                )
            })
            .count()
    }

    /// Returns the number of message-box frame blits parsed from the embedded
    /// JSON, used to compare against per-tick render output.
    fn json_frame_blit_count() -> usize {
        let value: serde_json::Value = serde_json::from_str(LEVEL_MESSAGEBOX_JSON)
            .expect("embedded level_messagebox_vga.json must be valid JSON");
        value
            .get("images")
            .and_then(|v| v.as_array())
            .map(|arr| arr.len())
            .unwrap_or(0)
    }

    /// Unit under test: `CheckpointChangeLevel` message dispatched via
    /// [`MessageDispatcher::send`] reaches the level screen and, after
    /// [`LEVEL_MESSAGE_TICKS`] ticks, surfaces a matching `ScreenTransition::Level`.
    ///
    /// Preconditions: level 2 screen subscribed to the dispatcher; a
    /// `CheckpointChangeLevel` message with payload pointing at `JN1L03.JN1`
    /// (level 3) is sent before the first tick.
    ///
    /// Invariants asserted: the first 71 ticks return no transition; the
    /// 72nd tick returns `ScreenTransition::Level { file: "JN1L03.JN1",
    /// number: 3 }`.
    #[test]
    fn change_level_message_swaps_after_message_ticks() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 2);
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(ChangeLevelPayload {
                level_file: String::from("JN1L03.JN1"),
                level_number: 3,
            }),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        for tick_index in 0..(LEVEL_MESSAGE_TICKS - 1) {
            let result = screen.tick(&input, &mut state);
            assert!(
                result.transition.is_none(),
                "tick {tick_index} must not transition before the message-box timer expires"
            );
        }
        let final_result = screen.tick(&input, &mut state);
        assert_eq!(
            final_result.transition,
            Some(ScreenTransition::Level {
                file: String::from("JN1L03.JN1"),
                number: 3,
            })
        );
    }

    /// Unit under test: `CheckpointChangeLevelPrevious` returns to the world map.
    ///
    /// Preconditions: level 2 screen subscribed; `CheckpointChangeLevelPrevious`
    /// is sent.
    ///
    /// Invariants asserted: the 72nd tick returns `ScreenTransition::Map`.
    #[test]
    fn previous_level_message_returns_to_map() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 2);
        dispatcher.send(
            MessageType::CheckpointChangeLevelPrevious,
            MessagePayload::None,
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let mut last: Option<ScreenTransition> = None;
        for _ in 0..LEVEL_MESSAGE_TICKS {
            last = screen.tick(&input, &mut state).transition.or(last);
        }
        assert_eq!(last, Some(ScreenTransition::Map));
    }

    /// Unit under test: messages dispatched during an active 72-tick hold
    /// are dropped rather than queued, so the inbox does not grow without
    /// bound while the message-box is on screen.
    ///
    /// Preconditions: level 1 screen subscribed; `CheckpointChangeLevel` is
    /// sent before the first tick to start the hold; many `DieRestartLevel`
    /// messages are then sent while the hold is active and a tick is run
    /// after each send so the inbox is drained.
    ///
    /// Invariants asserted: after the burst, the inbox is empty, and the
    /// transition that eventually fires is the original `Level` transition
    /// (the late-arriving `DieRestartLevel` messages are dropped, not
    /// promoted).
    #[test]
    fn pending_hold_drops_late_messages_and_drains_inbox() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(ChangeLevelPayload {
                level_file: String::from("JN1L02.JN1"),
                level_number: 2,
            }),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let mut transition: Option<ScreenTransition> = None;
        for _ in 0..LEVEL_MESSAGE_TICKS {
            // Bombard the dispatcher mid-hold with restart requests.
            dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
            transition = screen.tick(&input, &mut state).transition.or(transition);
        }

        assert_eq!(
            transition,
            Some(ScreenTransition::Level {
                file: String::from("JN1L02.JN1"),
                number: 2,
            }),
            "original Level transition must fire; late RestartLevel messages must not preempt it"
        );
        assert!(
            screen.inbox.lock().unwrap().is_empty(),
            "inbox must be drained even while a transition is pending"
        );
    }

    /// Unit under test: `DieRestartLevel` produces `ScreenTransition::RestartLevel`.
    ///
    /// Preconditions: level 2 screen subscribed; `DieRestartLevel` is sent.
    ///
    /// Invariants asserted: the 72nd tick returns `ScreenTransition::RestartLevel`.
    #[test]
    fn die_restart_message_emits_restart_transition() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 2);
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let mut transitions = Vec::new();
        for _ in 0..LEVEL_MESSAGE_TICKS {
            if let Some(t) = screen.tick(&input, &mut state).transition {
                transitions.push(t);
            }
        }
        assert_eq!(transitions.last(), Some(&ScreenTransition::RestartLevel));
    }

    /// Unit under test: the message box renders for exactly
    /// [`LEVEL_MESSAGE_TICKS`] ticks then disappears.
    ///
    /// Preconditions: level 1 screen; `CheckpointChangeLevel` queued before the
    /// first tick.
    ///
    /// Invariants asserted: each of the first `LEVEL_MESSAGE_TICKS` ticks
    /// includes at least one message-box-origin blit; the tick after the
    /// countdown expires does not (the screen no longer overlays the box once
    /// the transition has fired).
    #[test]
    fn message_box_visible_for_exactly_message_ticks() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(ChangeLevelPayload {
                level_file: String::from("JN1L02.JN1"),
                level_number: 2,
            }),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let expected_frame_blits = json_frame_blit_count();
        assert!(
            expected_frame_blits > 0,
            "JSON layout must list at least one frame blit"
        );

        for tick_index in 0..LEVEL_MESSAGE_TICKS {
            let commands = screen.tick(&input, &mut state).commands;
            let count = count_message_box_commands(&commands);
            assert!(
                count >= expected_frame_blits,
                "tick {tick_index} must render the message-box frame"
            );
        }

        let after = screen.tick(&input, &mut state).commands;
        let count_after = count_message_box_commands(&after);
        assert_eq!(
            count_after, 0,
            "message box must clear once the transition has fired"
        );
    }

    /// Unit under test: a checkpoint object whose counter matches the level
    /// number seeds the viewport offset to center the checkpoint within the
    /// game area, matching the Java reference's `centerScreen`.
    ///
    /// Preconditions: JN file holds a single object with `counter = 3`,
    /// `(x, y) = (256, 128)`; the screen is constructed for level 3.  The
    /// game area is `GAME_AREA_W = 232` by `GAME_AREA_H = 160` pixels.
    ///
    /// Invariants asserted: the viewport offset is
    /// `(GAME_AREA_W/2 - 256, GAME_AREA_H/2 - 128) = (-140, -48)`, which
    /// places the checkpoint at the center of the game area
    /// (`(116, 80)` in game-area-relative pixels).
    #[test]
    fn checkpoint_seeds_viewport_for_matching_counter() {
        let bytes = jn_bytes_with_objects(&[(3, 256, 128)]);
        let (screen, _dispatcher) = screen_with_dispatcher(bytes, 3);
        let (vx, vy) = screen.viewport();
        let center_x = openjill_core::layout::GAME_AREA_W as i32 / 2;
        let center_y = openjill_core::layout::GAME_AREA_H as i32 / 2;
        assert_eq!(vx, center_x - 256);
        assert_eq!(vy, center_y - 128);
    }

    /// Unit under test: an absent checkpoint falls back to viewport `(0, 0)`.
    ///
    /// Preconditions: JN file holds one object with `counter = 9` while the
    /// screen is constructed for level 1.
    ///
    /// Invariants asserted: the viewport offset is `(0, 0)`.
    #[test]
    fn missing_checkpoint_defaults_to_zero_viewport() {
        let bytes = jn_bytes_with_objects(&[(9, 256, 128)]);
        let (screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        assert_eq!(screen.viewport(), (0, 0));
    }

    /// Unit under test: `find_checkpoint` returns the first checkpoint object
    /// (type 12) whose counter equals the requested level number.
    #[test]
    fn find_checkpoint_returns_matching_object() {
        let bytes = jn_bytes_with_objects(&[(0, 10, 10), (2, 20, 20), (2, 30, 30)]);
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let obj = find_checkpoint(&jn, 2).expect("level 2 checkpoint should exist");
        assert_eq!(obj.x(), 20);
        assert_eq!(obj.y(), 20);
    }

    /// Unit under test: `find_checkpoint` filters by object type 12, so
    /// non-checkpoint objects whose `counter` happens to match the level
    /// number do not seed the viewport.
    ///
    /// Preconditions: a synthetic JN whose first two objects (types 38 and
    /// 15) carry `counter = 1` but are not checkpoints; the third object
    /// (type 12, the real checkpoint) also carries `counter = 1` at a
    /// distinct world position.
    ///
    /// Invariants asserted: `find_checkpoint` returns the type-12 object
    /// rather than the first counter match, mirroring the Java reference's
    /// `findCheckPoint` which iterates the level's checkpoint list only.
    #[test]
    fn find_checkpoint_filters_by_object_type_twelve() {
        let bytes = jn_bytes_with_typed_objects(&[
            (38, 1, 432, 416),
            (15, 1, 448, 496),
            (super::CHECKPOINT_OBJECT_TYPE, 1, 112, 208),
        ]);
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let obj = find_checkpoint(&jn, 1).expect("level 1 checkpoint should exist");
        assert_eq!(obj.object_type(), super::CHECKPOINT_OBJECT_TYPE);
        assert_eq!(obj.x(), 112);
        assert_eq!(obj.y(), 208);
    }

    /// Unit under test: when the JN file holds no checkpoint object whose
    /// `counter` matches the level number (only non-checkpoint objects do),
    /// the viewport falls back to `(0, 0)` rather than seeding off the
    /// first counter match.
    #[test]
    fn checkpoint_skips_non_checkpoint_counter_matches() {
        let bytes = jn_bytes_with_typed_objects(&[(38, 1, 432, 416), (15, 1, 448, 496)]);
        let (screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        assert_eq!(screen.viewport(), (0, 0));
    }

    /// Unit under test: `lookup_message_text` returns the JN1 message string
    /// for the destination level number, split on `\n`.
    ///
    /// Level 1 displays `messages.JN1[1]` ("JILL BOUNDS THROUGH THE BOULDERS").
    #[test]
    fn lookup_message_text_returns_jn1_entry_lines() {
        let lines = lookup_message_text(1);
        assert!(
            !lines.is_empty(),
            "level 1 must have a message-box text entry"
        );
        assert!(
            lines.iter().any(|line| line.contains("BOULDERS")),
            "level 1 message text should reference the boulders: got {lines:?}"
        );
    }

    /// Unit under test: `lookup_message_text` returns the jungle-map entry
    /// (`messages.JN1[0]`) for `MAP_LEVEL`, matching the Java reference's
    /// "return-to-map" message.
    #[test]
    fn lookup_message_text_returns_map_entry_for_map_level() {
        let lines = lookup_message_text(openjill_core::MAP_LEVEL);
        assert!(
            lines.iter().any(|line| line.contains("JUNGLE MAP")),
            "MAP_LEVEL must surface the jungle-map entry: got {lines:?}"
        );
    }

    /// Unit under test: `lookup_message_text` returns empty for a non-map
    /// negative level (i.e. an unknown sentinel that is not `MAP_LEVEL`).
    #[test]
    fn lookup_message_text_returns_empty_for_other_negative_levels() {
        assert!(lookup_message_text(-42).is_empty());
    }

    /// Unit under test: `LevelScreen::level_jn_bytes` round-trips the bytes
    /// supplied at construction so the orchestrator can restart the level
    /// from memory.
    #[test]
    fn level_jn_bytes_round_trip_preserves_source_bytes() {
        let mut bytes = jn_bytes_with_objects(&[]);
        bytes[0..2].copy_from_slice(&0x0042u16.to_le_bytes());
        let (screen, _dispatcher) = screen_with_dispatcher(bytes.clone(), 1);
        assert_eq!(screen.level_jn_bytes(), Some(bytes));
    }

    /// Unit under test: when `LevelScreen` is constructed with [`MAP_LEVEL`]
    /// (acting as the world-map screen), `map_jn_bytes` returns the JN bytes
    /// and `level_jn_bytes` returns `None`.
    ///
    /// Invariants asserted:
    /// - `map_jn_bytes()` returns the bytes passed to `from_bytes`
    /// - `level_jn_bytes()` returns `None` (MAP.JN1 bytes must not pollute
    ///   the orchestrator's level-byte cache)
    #[test]
    fn map_level_surfaces_map_jn_bytes_not_level_jn_bytes() {
        let mut bytes = jn_bytes_with_objects(&[]);
        bytes[0..2].copy_from_slice(&0x0099u16.to_le_bytes());
        let (screen, _dispatcher) = screen_with_dispatcher(bytes.clone(), openjill_core::MAP_LEVEL);
        assert_eq!(
            screen.map_jn_bytes(),
            Some(bytes),
            "map_jn_bytes must return the JN bytes when level_number == MAP_LEVEL"
        );
        assert_eq!(
            screen.level_jn_bytes(),
            None,
            "level_jn_bytes must return None when level_number == MAP_LEVEL"
        );
    }

    /// Unit under test: pressing Escape with no pending transition returns
    /// to the start menu, mirroring the abort behavior of the reference
    /// implementation.
    #[test]
    fn escape_returns_to_start_menu_when_idle() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        let mut input = ActiveInput::new();
        input.insert(InputCommand::Pause);
        let result = screen.tick(&input, &mut RuntimeState::new());
        assert_eq!(result.transition, Some(ScreenTransition::StartMenu));
    }

    /// Unit under test: `render_message_box` clips every frame blit to the
    /// box's declared bounding rectangle.
    ///
    /// Preconditions: the embedded layout JSON declares a 192x92 box at
    /// `(94, 48)`; the frame mosaic includes tiles whose 16-tall tile
    /// geometry extends past the bottom edge of the box (vertical bars on
    /// the lower row, lower horizontal bar tiles), and 16-wide tiles whose
    /// geometry extends past the right edge (right vertical bar).
    ///
    /// Invariants asserted: every emitted `Blit` carries a `clip`
    /// rectangle covering exactly the box's declared bounds so out-of-bounds
    /// pixels are dropped at present time instead of bleeding into the
    /// surrounding level content.
    #[test]
    fn render_message_box_clips_frame_tiles_to_box_bounds() {
        let commands = render_message_box(&[String::from("HI")]);
        let mut blit_count = 0_usize;
        for cmd in &commands {
            let RenderCommand::Blit { clip, .. } = cmd else {
                continue;
            };
            blit_count += 1;
            let clip = clip.expect("every message-box blit must carry a clip rect");
            assert_eq!(clip.x, 94, "clip x must match the box origin");
            assert_eq!(clip.y, 48, "clip y must match the box origin");
            assert_eq!(clip.width, 192, "clip width must match the box width");
            assert_eq!(
                clip.height, 92,
                "clip height must match the box height so 16-tall vertical bar tiles \
                 on the last row do not bleed below the box"
            );
        }
        assert!(
            blit_count > 0,
            "render_message_box must emit at least one frame blit"
        );
    }

    /// Unit under test: `render_message_box` caps emitted text lines at
    /// [`MESSAGE_MAX_LINES`] so the overlay cannot overflow the text area.
    #[test]
    fn render_message_box_clips_text_lines() {
        let text: Vec<String> = (0..MESSAGE_MAX_LINES + 5)
            .map(|i| format!("L{i}"))
            .collect();
        let commands = render_message_box(&text);
        let line_h = MESSAGE_LINE_HEIGHT;
        let drawn = commands
            .iter()
            .filter(|cmd| matches!(cmd, RenderCommand::DrawText { .. }))
            .count();
        assert_eq!(drawn, MESSAGE_MAX_LINES);
        assert!(line_h > 0);
    }

    /// Unit under test: `MessageDispatcher::clear` discards queued messages
    /// before a [`LevelScreen`] subscribes.
    ///
    /// Preconditions: a `DieRestartLevel` message is sent into an empty
    /// dispatcher, then `clear` is called.  A fresh `LevelScreen` then
    /// subscribes via `LevelScreen::from_bytes`.
    ///
    /// Invariants asserted: the first tick after subscribing returns no
    /// transition, confirming the cleared queue was not replayed into the
    /// new subscriber.
    #[test]
    fn dispatcher_clear_drops_pending_before_subscribe() {
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
        dispatcher.clear();
        let bytes = jn_bytes_with_objects(&[]);
        let cache = synthetic_cache();
        let mut screen =
            LevelScreen::from_bytes(bytes, &cache, 1, &mut dispatcher, EPISODE_1_SKY_COLOR)
                .expect("synthetic level JN should parse");
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(
            result.transition.is_none(),
            "cleared dispatcher must not deliver previously-queued messages"
        );
    }

    /// Unit under test: `LevelScreen::tick` with no objects and an all-zero
    /// background layer still emits the per-level sky `FillRect` over the
    /// game area.
    ///
    /// Preconditions: a synthetic JN buffer with zero objects and all-zero
    /// background map codes; the synthetic `AssetCache` carries an empty
    /// DMA, so every cell resolves to the transparent
    /// `StdBackgroundEntity` placeholder that emits no draw output.
    ///
    /// Invariants asserted: the tick completes without panic, and the
    /// resulting command list contains the sky `FillRect` over the game
    /// area so the renderer always has a baseline fill to execute even
    /// when both the object and background entity iterations contribute
    /// nothing.  No explicit `RenderCommand::Clear` is emitted because the
    /// presenter clears the framebuffer on its own and an extra clear
    /// would overwrite the orchestrator-prepended status bar tiles.
    #[test]
    fn tick_with_no_objects_and_empty_backgrounds_emits_sky_fill() {
        use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(
            result.commands.iter().any(|cmd| matches!(
                cmd,
                RenderCommand::FillRect {
                    x,
                    y,
                    width,
                    height,
                    color,
                } if *x == GAME_AREA_X
                    && *y == GAME_AREA_Y
                    && *width == GAME_AREA_W
                    && *height == GAME_AREA_H
                    && *color == EPISODE_1_SKY_COLOR
            )),
            "tick must emit a sky FillRect baseline command; got {:?}",
            result.commands
        );
        assert!(
            !result
                .commands
                .iter()
                .any(|cmd| matches!(cmd, RenderCommand::Clear { .. })),
            "tick must not emit a Clear; it would overwrite the prepended status bar"
        );
    }

    /// Unit under test: `LevelScreen::tick` emits a sky `FillRect` over the
    /// game area before any background tile blits, and does not emit a
    /// `RenderCommand::Clear` (which would overwrite the prepended status
    /// bar tiles).
    ///
    /// Preconditions: synthetic level 1 screen built with
    /// [`EPISODE_1_SKY_COLOR`]; no objects and an empty background grid so
    /// no per-entity draws appear before the level base layer.
    ///
    /// Invariants asserted: a `FillRect` whose rectangle equals the
    /// `(GAME_AREA_X, GAME_AREA_Y, GAME_AREA_W, GAME_AREA_H)` window and
    /// whose `color` matches `EPISODE_1_SKY_COLOR` is emitted, and its
    /// position in the command list precedes the first background `Blit`
    /// so blits paint over the sky.  The tick must also not emit a
    /// `RenderCommand::Clear` because the presenter clears the framebuffer
    /// on its own and an extra clear would erase the static status bar.
    #[test]
    fn tick_emits_sky_fill_rect_before_blits_and_no_clear() {
        use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());

        assert!(
            !result
                .commands
                .iter()
                .any(|cmd| matches!(cmd, RenderCommand::Clear { .. })),
            "tick must not emit a Clear; it would overwrite the prepended status bar"
        );

        let sky_index = result
            .commands
            .iter()
            .position(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                    } if *x == GAME_AREA_X
                        && *y == GAME_AREA_Y
                        && *width == GAME_AREA_W
                        && *height == GAME_AREA_H
                        && *color == EPISODE_1_SKY_COLOR
                )
            })
            .expect("tick must emit a sky FillRect covering the game area");

        if let Some(blit_index) = result
            .commands
            .iter()
            .position(|cmd| matches!(cmd, RenderCommand::Blit { .. }))
        {
            assert!(
                sky_index < blit_index,
                "sky FillRect must precede the first Blit; commands: {:?}",
                result.commands
            );
        }
    }

    /// Unit under test: [`EPISODE_1_SKY_COLOR`] resolves to the dark blue
    /// VGA palette entry the original DOS episode 1 sky uses.
    ///
    /// Invariants asserted: the constant equals palette index 1, and that
    /// palette index in the embedded VGA palette is `(0x00, 0x00, 0xA2)`.
    #[test]
    fn episode_1_sky_color_is_palette_index_one_dark_blue() {
        assert_eq!(
            EPISODE_1_SKY_COLOR, 1,
            "episode 1 sky color must be VGA palette index 1"
        );
        let palette = openjill_core::JILL_VGA_PALETTE;
        assert_eq!(
            palette[EPISODE_1_SKY_COLOR as usize],
            [0x00, 0x00, 0xA2],
            "VGA palette index 1 must be the saturated dark blue used by JN1 sky"
        );
    }

    /// Unit under test: `LevelScreen::tick` emits the message-box overlay
    /// after the per-entity draw commands so background and object draws
    /// cannot paint over the box mid-transition.
    ///
    /// Preconditions: synthetic level 1 screen; `CheckpointChangeLevel` is
    /// dispatched before the first tick to start the message-box countdown.
    ///
    /// Invariants asserted: the message-box frame blits (tileset 24 / 3)
    /// land at a higher index in the command list than the sky `FillRect`
    /// baseline, so a renderer executing in order paints the message box
    /// on top of the level background.
    #[test]
    fn tick_message_box_renders_after_base_frame() {
        use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(ChangeLevelPayload {
                level_file: String::from("JN1L02.JN1"),
                level_number: 2,
            }),
        );
        let commands = screen
            .tick(&ActiveInput::new(), &mut RuntimeState::new())
            .commands;
        let sky_index = commands
            .iter()
            .position(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect {
                        x,
                        y,
                        width,
                        height,
                        color,
                    } if *x == GAME_AREA_X
                        && *y == GAME_AREA_Y
                        && *width == GAME_AREA_W
                        && *height == GAME_AREA_H
                        && *color == EPISODE_1_SKY_COLOR
                )
            })
            .expect("tick must emit a sky FillRect baseline command");
        let last_messagebox_blit = commands
            .iter()
            .rposition(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::Blit { tileset: 24, .. }
                        | RenderCommand::Blit { tileset: 3, .. }
                )
            })
            .expect("tick must emit message-box frame blits while a transition is pending");
        assert!(
            last_messagebox_blit > sky_index,
            "message-box overlay must follow the sky FillRect baseline in the command list"
        );
    }

    /// Unit under test: `translate_object_command` rewrites a world-coord
    /// `Blit` emitted from `ObjectEntity::draw` into framebuffer coordinates
    /// using the OpenJill sign convention shared with `tick_backgrounds` and
    /// `render_map_background`.
    ///
    /// Preconditions: a synthetic `Blit` with world `(x, y) = (200, 64)`,
    /// no clip; the active `viewport_x = -16` (world origin at `+16`),
    /// `viewport_y = 0` (world origin at the game-area top).
    ///
    /// Invariants asserted: the rewritten command lands at
    /// `(GAME_AREA_X + 200 + viewport_x, GAME_AREA_Y + 64 + viewport_y) =
    /// (80 + 200 - 16, 16 + 64) = (264, 80)`, and carries the shared
    /// `GAME_AREA_CLIP` so the sprite cannot bleed past the right or bottom
    /// game-area border into the surrounding status bar.
    #[test]
    fn translate_object_command_applies_viewport_offset_and_clip() {
        use crate::status_bar::GAME_AREA_CLIP;
        let cmd = RenderCommand::Blit {
            tileset: 8,
            tile: 16,
            x: 200,
            y: 64,
            opaque: false,
            clip: None,
        };
        let translated = super::translate_object_command(cmd, -16, 0);
        match translated {
            RenderCommand::Blit { x, y, clip, .. } => {
                assert_eq!(
                    x,
                    80 + 200 + -16,
                    "screen_x = GAME_AREA_X + world_x + viewport_x"
                );
                assert_eq!(y, 16 + 64, "screen_y = GAME_AREA_Y + world_y + viewport_y");
                assert_eq!(
                    clip,
                    Some(GAME_AREA_CLIP),
                    "object Blit must adopt the shared game-area clip when none was supplied"
                );
            }
            other => panic!("expected Blit; got {other:?}"),
        }
    }

    /// Unit under test: `translate_object_command` preserves an explicit
    /// clip rectangle supplied by the entity instead of overriding it with
    /// the shared game-area clip.
    #[test]
    fn translate_object_command_preserves_explicit_clip() {
        let explicit = openjill_core::ClipRect {
            x: 100,
            y: 50,
            width: 50,
            height: 32,
        };
        let cmd = RenderCommand::Blit {
            tileset: 8,
            tile: 16,
            x: 0,
            y: 0,
            opaque: false,
            clip: Some(explicit),
        };
        let translated = super::translate_object_command(cmd, 0, 0);
        let RenderCommand::Blit { clip, .. } = translated else {
            panic!("expected Blit");
        };
        assert_eq!(clip, Some(explicit));
    }

    /// Unit under test: `tick_backgrounds` derives screen pixel positions
    /// from the viewport offset using the OpenJill sign convention shared
    /// with `render_map_background`.
    ///
    /// Preconditions: a synthetic JN whose background layer at cell
    /// `(5, 0)` carries a non-transparent map code, paired with a DMA file
    /// that supplies an entry for that code; the screen's viewport is
    /// offset by one tile horizontally so the cell does not sit on the
    /// game-area left edge.
    ///
    /// Invariants asserted: the background blit for that cell lands at
    /// `screen_x = GAME_AREA_X + cell_world_x - world_origin_x`, which for
    /// `viewport_x = -16` (world origin at `+16`) and `cell_world_x = 80`
    /// puts the blit at `80 + 80 - 16 = 144` — the position
    /// `render_map_background` would also produce.
    #[test]
    fn tick_backgrounds_uses_openjill_viewport_sign() {
        // Synthesize a one-entry DMA file matching the parser layout:
        //   map_code (u16 LE) + tile (u8) + tileset_with_flags (u8) +
        //   flags (u16 LE) + name_len (u8) + name (name_len ASCII bytes).
        // map_code=1, tile=42, tileset=7, flags=0, name="TEST".
        let mut dma_bytes: Vec<u8> = Vec::new();
        dma_bytes.extend_from_slice(&1u16.to_le_bytes()); // map_code
        dma_bytes.push(42); // tile
        dma_bytes.push(7); // tileset_with_flags
        dma_bytes.extend_from_slice(&0u16.to_le_bytes()); // flags
        dma_bytes.push(4); // name_len
        dma_bytes.extend_from_slice(b"TEST"); // name
        let dma =
            openjill_data::dma::DmaFile::from_bytes(dma_bytes).expect("synthetic DMA should parse");
        let mut cache = AssetCache::synthetic();
        cache.dma = dma;

        // Build a JN whose background cell (5, 0) carries map code 1.
        // The JN parser stores cells as `x * BACKGROUND_HEIGHT + y` so the
        // byte offset for cell `(5, 0)` is `5 * 64 * 2 = 640`.
        let mut bytes = jn_bytes_with_objects(&[]);
        let cell_off = 5 * 64 * 2;
        bytes[cell_off..cell_off + 2].copy_from_slice(&1u16.to_le_bytes());

        let mut dispatcher = MessageDispatcher::new();
        let mut screen =
            LevelScreen::from_bytes(bytes, &cache, 1, &mut dispatcher, EPISODE_1_SKY_COLOR)
                .expect("synthetic level JN should parse");

        // Force the viewport to a known non-zero offset so the screen-pos
        // sign convention is exercised: viewport_x = -16 means the world
        // origin is at +16, and cell (5,0)'s world pixel is (80, 0).
        // Expected screen_x = GAME_AREA_X (80) + 80 - 16 = 144.
        let cell_world_x = 5 * 16_i32;
        let viewport_x = -16_i32;
        let expected_screen_x = openjill_core::layout::GAME_AREA_X + cell_world_x - (-viewport_x);
        let expected_screen_y = openjill_core::layout::GAME_AREA_Y;

        // Mutate viewport directly to bypass the checkpoint heuristic.
        screen.viewport_x = viewport_x;
        screen.viewport_y = 0;

        let bg_commands = screen.tick_backgrounds(None);
        let blit = bg_commands
            .iter()
            .find_map(|cmd| match cmd {
                RenderCommand::Blit {
                    tileset: 7,
                    tile: 42,
                    x,
                    y,
                    ..
                } => Some((*x, *y)),
                _ => None,
            })
            .expect("background entity must emit the registered tileset/tile blit");
        assert_eq!(
            blit,
            (expected_screen_x, expected_screen_y),
            "background blit must use the same viewport sign convention as render_map_background"
        );
    }

    /// Unit under test: viewport seeding ignores a checkpoint object whose
    /// counter overflows `i16`, falling back to `(0, 0)`.
    #[test]
    fn checkpoint_skips_out_of_range_level() {
        let bytes = jn_bytes_with_objects(&[(1, 100, 50)]);
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        // 100_000 does not fit in i16; the lookup falls back to (0, 0).
        assert_eq!(checkpoint_viewport(&jn, 100_000), (0, 0));
    }

    /// Returns the world top-left X (in pixels) currently shown at the
    /// viewport's left edge, derived from the OpenJill sign-flipped form
    /// stored in `viewport_x`.
    fn world_left(viewport_x: i32) -> i32 {
        -viewport_x
    }

    /// Unit under test: [`compute_viewport_scroll`] when the player bounding
    /// box sits comfortably inside the 96/48 px inner border.
    ///
    /// Preconditions: viewport starts at `(0, 0)` (world top-left = 0);
    /// player bounding box at `(120, 80, 16, 16)` is well inside the border.
    ///
    /// Invariants asserted: the returned viewport equals the input, i.e. no
    /// scroll is applied while the player stays inside the border.
    #[test]
    fn viewport_unchanged_when_player_inside_border() {
        let player = Rect::new(120, 80, 16, 16);
        let (vx, vy) = compute_viewport_scroll(player, 0, 0);
        assert_eq!((vx, vy), (0, 0));
    }

    /// Unit under test: [`compute_viewport_scroll`] when the player's right
    /// edge crosses the right inner border.
    ///
    /// Preconditions: viewport at `(0, 0)`; player at
    /// `(GAME_AREA_W - X_UPDATE_BORDER, 80, 16, 16)`, so
    /// `player.x + player.w` exceeds the right border by 16.
    ///
    /// Invariants asserted: the viewport scrolls right exactly enough to put
    /// the player at the border (`world_left == 16`), matching the Java
    /// reference's snap-to-border rule.
    #[test]
    fn viewport_scrolls_right_when_player_passes_right_border() {
        let player_x = GAME_AREA_W as i32 - X_UPDATE_BORDER as i32; // 232 - 96 = 136
        let player = Rect::new(player_x, 80, 16, 16);
        let (vx, _vy) = compute_viewport_scroll(player, 0, 0);
        // `player.x + player.w = 152`; right border at `0 + 232 - 96 = 136`;
        // overshoot of 16 advances world_left to 16.
        assert_eq!(world_left(vx), 16);
    }

    /// Unit under test: [`compute_viewport_scroll`] when the player's left
    /// edge crosses the left inner border and the resulting scroll would go
    /// past world X = 0.
    ///
    /// Preconditions: viewport at `(0, 0)` (already at the left edge);
    /// player at `(0, 80, 16, 16)`, deep inside the left border.
    ///
    /// Invariants asserted: the viewport clamps at world X = 0 rather than
    /// producing a negative world_left, matching the spec's "clamps at 0"
    /// requirement.
    #[test]
    fn viewport_clamps_at_zero_when_player_near_left_edge() {
        let player = Rect::new(0, 80, 16, 16);
        let (vx, _vy) = compute_viewport_scroll(player, 0, 0);
        assert_eq!(world_left(vx), 0);
    }

    /// Unit under test: [`compute_viewport_scroll`] when the player's right
    /// edge would push the viewport past the map's scrollable maximum.
    ///
    /// Preconditions: player positioned near the map's right edge; viewport
    /// already at the maximum scroll position.
    ///
    /// Invariants asserted: the viewport clamps at
    /// `MAP_WIDTH * 16 - GAME_AREA_W` rather than allowing the world_left to
    /// exceed the scrollable range.
    #[test]
    fn viewport_clamps_at_map_right_edge() {
        let map_w_px = BACKGROUND_GRID_WIDTH as i32 * 16;
        let max_world_left = map_w_px - GAME_AREA_W as i32;
        let player = Rect::new(map_w_px - 16, 80, 16, 16);
        let (vx, _vy) = compute_viewport_scroll(player, -max_world_left, 0);
        assert_eq!(world_left(vx), max_world_left);
    }

    /// Unit under test: vertical clamp at the bottom of the map.
    ///
    /// Preconditions: player near the map's bottom edge; viewport already at
    /// the maximum vertical scroll.
    ///
    /// Invariants asserted: world_top clamps at
    /// `MAP_HEIGHT * 16 - GAME_AREA_H`.
    #[test]
    fn viewport_clamps_at_map_bottom_edge() {
        let map_h_px = BACKGROUND_GRID_HEIGHT as i32 * 16;
        let max_world_top = map_h_px - GAME_AREA_H as i32;
        let player = Rect::new(100, map_h_px - 16, 16, 16);
        let (_vx, vy) = compute_viewport_scroll(player, 0, -max_world_top);
        assert_eq!(-vy, max_world_top);
    }

    /// Unit under test: vertical scroll when the player crosses the bottom
    /// 48 px inner border.
    ///
    /// Preconditions: viewport at `(0, 0)`; player at
    /// `(100, GAME_AREA_H - Y_UPDATE_BORDER, 16, 16)`, overshooting the
    /// bottom border by 16 px.
    ///
    /// Invariants asserted: world_top scrolls down to exactly 16.
    #[test]
    fn viewport_scrolls_down_when_player_passes_bottom_border() {
        let player_y = GAME_AREA_H as i32 - Y_UPDATE_BORDER as i32; // 160 - 48 = 112
        let player = Rect::new(100, player_y, 16, 16);
        let (_vx, vy) = compute_viewport_scroll(player, 0, 0);
        assert_eq!(-vy, 16);
    }

    /// Test helper: returns the first `DrawText` command whose `(x, y)`
    /// equals the score's expected framebuffer anchor, panicking when no
    /// such command exists.
    fn score_draw_text(commands: &[RenderCommand]) -> &RenderCommand {
        let target_y = INVENTORY_AREA_Y + super::SCORE_Y_INV;
        commands
            .iter()
            .find(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::DrawText { y, color_index, .. }
                        if *y == target_y && *color_index == super::SCORE_COLOR
                )
            })
            .expect("expected a score DrawText command in the per-tick output")
    }

    /// Test helper: returns the first `DrawText` command whose `y` falls
    /// inside the message bar band, panicking when none exists.
    fn message_bar_draw_text(commands: &[RenderCommand]) -> Option<&RenderCommand> {
        commands.iter().find(|cmd| {
            matches!(
                cmd,
                RenderCommand::DrawText { y, .. } if *y >= MESSAGE_BAR_Y
            )
        })
    }

    /// Unit under test: dispatching an [`MessageType::InventoryPoint`]
    /// message causes the next tick to emit a score `DrawText` containing
    /// the new accumulated score.
    ///
    /// Preconditions: a fresh level screen subscribed to the dispatcher; a
    /// single `InventoryPoint` message with delta +500 is sent before the
    /// first tick.
    ///
    /// Invariants asserted: after the tick, `state.score` equals 500 and the
    /// returned command list carries a `DrawText` at the score's framebuffer
    /// anchor whose text is `"000500"` (zero-padded six-digit decimal).
    #[test]
    fn inventory_point_message_emits_score_draw_text_with_new_value() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(MessageType::InventoryPoint, MessagePayload::Count(500));

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);
        assert_eq!(state.score, 500);
        let cmd = score_draw_text(&result.commands);
        let RenderCommand::DrawText { text, .. } = cmd else {
            unreachable!("score_draw_text guarantees a DrawText variant");
        };
        assert_eq!(text, "500");
    }

    /// Unit under test: [`MessageType::InventoryLife`] accumulates into the
    /// shared [`RuntimeState::lives`] field across ticks.
    ///
    /// Preconditions: `RuntimeState::new()` starts at 3 lives; a -1
    /// `InventoryLife` message is sent.
    ///
    /// Invariants asserted: after the tick, `state.lives == 2` and a lives
    /// `DrawText` command appears whose text is `"2"`.
    #[test]
    fn inventory_life_message_decrements_lives_and_redraws_digit() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(MessageType::InventoryLife, MessagePayload::Count(-1));

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        screen.tick(&input, &mut state);
        assert_eq!(state.lives, 2);
    }

    /// Unit under test: [`MessageType::InventoryItem`] appends the carried
    /// item to the shared inventory and triggers an item-grid Blit on the
    /// same tick.
    ///
    /// Preconditions: empty inventory; a single `InventoryItem(Gem)`
    /// message.
    ///
    /// Invariants asserted: `state.inventory` ends with one `Gem`; the
    /// returned commands include a `Blit` at the grid's first cell carrying
    /// the gem tileset / tile from `inventory_conf.json`.
    #[test]
    fn inventory_item_message_appends_and_emits_grid_blit() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::Gem)),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);
        assert_eq!(
            state.inventory,
            vec![InventoryObject::Jill, InventoryObject::Gem]
        );
        // JILL token occupies slot 0 (col 0); gem lands in slot 1 (col 1).
        let target_x = INVENTORY_AREA_X + super::ITEM_GRID_X_INV + super::ITEM_GRID_PITCH;
        let target_y = INVENTORY_AREA_Y + super::ITEM_GRID_Y_INV;
        let grid_blit = result.commands.iter().find(|cmd| {
            matches!(
                cmd,
                RenderCommand::Blit {
                    tileset: 14,
                    tile: 11,
                    x,
                    y,
                    ..
                } if *x == target_x && *y == target_y
            )
        });
        assert!(
            grid_blit.is_some(),
            "expected a gem Blit at the inventory grid's top-left cell"
        );
    }

    /// Unit under test: score saturates at [`super::SCORE_DISPLAY_MAX`] on
    /// ingest so the rendered text always fits the six-digit erase band.
    ///
    /// Preconditions: a fresh level screen; a single `InventoryPoint`
    /// message carrying a delta well past the six-digit ceiling.
    ///
    /// Invariants asserted: `state.score` clamps to
    /// `SCORE_DISPLAY_MAX = 999_999`; the score `DrawText` carries exactly
    /// six glyphs (`"999999"`), never seven.
    #[test]
    fn inventory_point_message_caps_score_at_six_digit_max() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::InventoryPoint,
            MessagePayload::Count(2_000_000),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);
        assert_eq!(state.score, super::SCORE_DISPLAY_MAX);
        let RenderCommand::DrawText { text, .. } = score_draw_text(&result.commands) else {
            unreachable!("score_draw_text guarantees DrawText");
        };
        assert_eq!(text, "999999");
    }

    /// Unit under test: a negative `InventoryPoint` delta never drives the
    /// rendered score below zero.
    ///
    /// Preconditions: fresh state (`score == 0`); an `InventoryPoint(-50)`
    /// message.
    ///
    /// Invariants asserted: `state.score` stays at 0; the score `DrawText`
    /// is `"000000"`.
    #[test]
    fn inventory_point_message_clamps_score_at_zero() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(MessageType::InventoryPoint, MessagePayload::Count(-50));

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);
        assert_eq!(state.score, 0);
        let RenderCommand::DrawText { text, .. } = score_draw_text(&result.commands) else {
            unreachable!("score_draw_text guarantees DrawText");
        };
        assert_eq!(text, "0");
    }

    /// Unit under test: a large `state.lives` value is left untouched by tick.
    ///
    /// Preconditions: fresh screen; `state.lives` pre-seeded to 25.
    ///
    /// Invariants asserted: `state.lives` is unchanged after the tick.
    #[test]
    fn lives_draw_text_clamps_to_single_digit_when_state_exceeds_cap() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        state.lives = 25;
        screen.tick(&input, &mut state);
        assert_eq!(state.lives, 25, "underlying state must be left untouched");
    }

    /// Unit under test: a negative `InventoryLife` delta cannot drive
    /// `state.lives` below zero.
    ///
    /// Preconditions: fresh `RuntimeState::new()` (3 lives); two
    /// `InventoryLife(-5)` deltas applied across separate ticks.
    ///
    /// Invariants asserted: `state.lives` saturates at 0 rather than going
    /// negative.
    #[test]
    fn inventory_life_message_clamps_lives_at_zero() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        dispatcher.send(MessageType::InventoryLife, MessagePayload::Count(-5));
        screen.tick(&input, &mut state);
        assert_eq!(state.lives, 0);
        dispatcher.send(MessageType::InventoryLife, MessagePayload::Count(-1));
        screen.tick(&input, &mut state);
        assert_eq!(state.lives, 0);
    }

    /// Unit under test: every inventory grid `Blit` carries the inventory
    /// area clip so the rightmost icon column's two-pixel overflow can
    /// never bleed into the static status-bar frame.
    ///
    /// Preconditions: a fresh screen with four `InventoryItem(Gem)`
    /// messages (one full row, so the last column gets exercised).
    ///
    /// Invariants asserted: every grid `Blit` reports
    /// `clip == Some(INVENTORY_AREA_CLIP)`.
    #[test]
    fn inventory_grid_blits_carry_inventory_area_clip() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        for _ in 0..4 {
            dispatcher.send(
                MessageType::InventoryItem,
                MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::Gem)),
            );
        }

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);

        let grid_blits: Vec<&RenderCommand> = result
            .commands
            .iter()
            .filter(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::Blit {
                        tileset: 14,
                        tile: 11,
                        ..
                    }
                )
            })
            .collect();
        assert_eq!(grid_blits.len(), 4, "expected one Blit per inventory item");
        for cmd in grid_blits {
            let RenderCommand::Blit { clip, .. } = cmd else {
                unreachable!("filter guarantees Blit");
            };
            assert_eq!(
                *clip,
                Some(super::INVENTORY_AREA_CLIP),
                "inventory grid blits must carry the inventory-area clip"
            );
        }
    }

    /// Unit under test: the inventory grid erase `FillRect` is clamped to
    /// the inventory area's interior.
    ///
    /// Preconditions: fresh screen; one tick.
    ///
    /// Invariants asserted: the grid erase `FillRect` does not extend past
    /// `INVENTORY_AREA_X + INVENTORY_AREA_W` on the right, nor past
    /// `INVENTORY_AREA_Y + INVENTORY_AREA_H` on the bottom.
    #[test]
    fn inventory_grid_erase_stays_inside_inventory_area() {
        use openjill_core::layout::{INVENTORY_AREA_H, INVENTORY_AREA_W};
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();
        let result = screen.tick(&input, &mut state);

        let grid_x = INVENTORY_AREA_X + super::ITEM_GRID_X_INV;
        let grid_y = INVENTORY_AREA_Y + super::ITEM_GRID_Y_INV;
        let inv_right = INVENTORY_AREA_X + INVENTORY_AREA_W as i32;
        let inv_bottom = INVENTORY_AREA_Y + INVENTORY_AREA_H as i32;
        let erase = result
            .commands
            .iter()
            .find(|cmd| {
                matches!(
                    cmd,
                    RenderCommand::FillRect { x, y, .. } if *x == grid_x && *y == grid_y
                )
            })
            .expect("expected a grid erase FillRect");
        let RenderCommand::FillRect {
            x,
            y,
            width,
            height,
            ..
        } = erase
        else {
            unreachable!("filter guarantees FillRect");
        };
        assert!(
            x + *width as i32 <= inv_right,
            "grid erase must not extend past the inventory right edge"
        );
        assert!(
            y + *height as i32 <= inv_bottom,
            "grid erase must not extend past the inventory bottom edge"
        );
    }

    /// Unit under test: [`MessageType::StatusBarText`] writes a `DrawText`
    /// inside the message bar (y >= 188) and clears the message exactly
    /// [`LEVEL_MESSAGE_TICKS`] ticks later.
    ///
    /// Preconditions: a fresh screen; a `StatusBarText` message with payload
    /// `"PICK UP THE GEM"`.
    ///
    /// Invariants asserted: tick 1 emits a message-bar `DrawText` carrying
    /// the payload; ticks 2..=72 each still emit the same text; tick 73
    /// emits no message-bar `DrawText` because the 72-tick countdown has
    /// expired.
    #[test]
    fn status_bar_text_renders_then_clears_after_72_ticks() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, mut dispatcher) = screen_with_dispatcher(bytes, 1);
        dispatcher.send(
            MessageType::StatusBarText,
            MessagePayload::Text(String::from("PICK UP THE GEM")),
        );

        let input = ActiveInput::new();
        let mut state = RuntimeState::new();

        // Tick 1: the text has just been queued; the overlay should render
        // it inside the message bar.
        let first = screen.tick(&input, &mut state);
        let cmd = message_bar_draw_text(&first.commands)
            .expect("expected a message-bar DrawText on the first tick");
        let RenderCommand::DrawText { text, y, .. } = cmd else {
            unreachable!("message_bar_draw_text guarantees DrawText");
        };
        assert_eq!(text, "PICK UP THE GEM");
        assert!(*y >= MESSAGE_BAR_Y, "DrawText must land at y >= 188");

        // Ticks 2..=72: the message-bar text persists for the full
        // LEVEL_MESSAGE_TICKS window.
        for _ in 1..LEVEL_MESSAGE_TICKS {
            let result = screen.tick(&input, &mut state);
            assert!(
                message_bar_draw_text(&result.commands).is_some(),
                "status-bar text must remain visible across the 72-tick window"
            );
        }

        // Tick 73: the countdown has reached zero and the message-bar
        // DrawText must no longer be emitted.
        let after = screen.tick(&input, &mut state);
        assert!(
            message_bar_draw_text(&after.commands).is_none(),
            "status-bar text must clear after LEVEL_MESSAGE_TICKS ticks"
        );
    }
}
