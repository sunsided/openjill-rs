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
    BLOCK_SIZE_I, GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y, LEVEL_MESSAGE_TICKS,
    X_UPDATE_BORDER, Y_UPDATE_BORDER,
};
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, BackgroundGrid, ChangeLevelPayload, InputCommand, MessageDispatcher,
    MessageHandler, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand, ScreenHandler,
    ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{JnFile, JnObject, JnReadError};

use crate::asset_cache::AssetCache;
use crate::entities::{make_background_entity, make_object_entity};
use crate::screens::map_screen::render_map_background;
use crate::status_bar::GAME_AREA_CLIP;

/// Embedded `level_messagebox_vga.json` layout resource from the Java reference port.
const LEVEL_MESSAGEBOX_JSON: &str =
    include_str!("../../../../OpenJill/src/main/resources/level_messagebox_vga.json");

/// Save prefix for the episode 1 messages table inside
/// [`LEVEL_MESSAGEBOX_JSON`]; the JSON also carries `JN2` and `JN3` entries that
/// are not yet exercised.
const EPISODE_SAVE_PREFIX: &str = "JN1";

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

        let (viewport_x, viewport_y) = checkpoint_viewport(&jn, level_number);
        let objects = build_object_entities(&jn, cache);
        let backgrounds = build_background_grid(&jn, cache);
        let dma = cache.dma.clone();

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
            entity_dispatcher: MessageDispatcher::new(),
            sky_color,
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

    /// Renders the base frame: the framebuffer-clear baseline, the per-level
    /// sky fill over the game area, and the static level background.
    ///
    /// The presenter already clears the framebuffer to palette index 0 every
    /// frame, so the leading [`RenderCommand::Clear`] here is redundant with
    /// it; it is retained as an explicit baseline so a downstream caller that
    /// executes the screen's commands against a buffer it did not clear still
    /// sees a deterministic starting state.  The [`RenderCommand::FillRect`]
    /// that follows fills only the game-area sub-region with [`self.sky_color`]
    /// so transparent map cells (map code 0) reveal the per-episode sky
    /// instead of the framebuffer's palette-index-0 clear.
    ///
    /// The message-box overlay is intentionally not included here; the tick
    /// loop appends it after the per-entity draw commands so the box paints
    /// on top of the level and any objects in front of it.
    fn render_base_frame(&self) -> Vec<RenderCommand> {
        let mut commands = vec![
            RenderCommand::Clear { color: 0 },
            RenderCommand::FillRect {
                x: GAME_AREA_X,
                y: GAME_AREA_Y,
                width: GAME_AREA_W,
                height: GAME_AREA_H,
                color: self.sky_color,
            },
        ];
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

    /// Advances every object entity by one tick and collects their render
    /// commands.
    ///
    /// An object updates when it reports `always_active` or when its
    /// bounding box overlaps the viewport expanded by [`X_UPDATE_BORDER`] and
    /// [`Y_UPDATE_BORDER`].  Objects whose bounding box overlaps the visible
    /// game-area window contribute their `draw` command.
    fn tick_objects(&mut self, input: &ActiveInput, state: &RuntimeState) -> Vec<RenderCommand> {
        let mut commands = Vec::new();
        let update_rect = viewport_update_rect(self.viewport_x, self.viewport_y);
        let game_rect = viewport_game_rect(self.viewport_x, self.viewport_y);
        for obj in self.objects.iter_mut() {
            let bbox = obj.bounding_box();
            if obj.always_active() || update_rect.intersects(&bbox) {
                obj.update(input, state, &self.backgrounds, &mut self.entity_dispatcher);
            }
            if game_rect.intersects(&bbox)
                && let Some(cmd) = obj.draw()
            {
                commands.push(translate_object_command(
                    cmd,
                    self.viewport_x,
                    self.viewport_y,
                ));
            }
        }
        commands
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

    /// Discards any messages queued in [`Self::entity_dispatcher`].
    ///
    /// Issue 57 has no entity-dispatcher subscribers; future child issues
    /// will replace this with the "drain object-removal messages" pass
    /// described in `docs/port/06-episode-1-gameplay.md`.
    fn drain_entity_dispatcher(&mut self) {
        self.entity_dispatcher.clear();
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

        // Render order each tick:
        // 1. Base frame (`Clear` + static level background).
        // 2. Per-cell background entity draws (overlay tiles).
        // 3. Object entity draws (drawn on top of backgrounds, mirroring
        //    `AbstractExecutingStdLevel` in the Java reference).
        // 4. Message-box overlay last so transitions paint over everything
        //    else.
        let mut commands = self.render_base_frame();

        // Update phase: objects update before backgrounds run their per-cell
        // callbacks so the `player_bbox` fed into the background loop reflects
        // the post-update player position rather than a stale pre-tick value.
        let obj_commands = self.tick_objects(input, state);
        let player_bbox = self.player_bounding_box();
        let bg_commands = self.tick_backgrounds(player_bbox);

        // Backgrounds first, then objects, matching the Java draw order.
        commands.extend(bg_commands);
        commands.extend(obj_commands);

        if self.pending.is_some() {
            commands.extend(render_message_box(&self.message_text));
        }

        self.drain_entity_dispatcher();

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

    /// Returns the raw level JN bytes preserved at construction so the
    /// orchestrator can reload the level from memory on restart.
    fn level_jn_bytes(&self) -> Option<Vec<u8>> {
        Some(self.jn_bytes.clone())
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
    jn.objects()
        .iter()
        .map(|obj| make_object_entity(obj.object_type(), obj, cache))
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
        MESSAGE_MAX_LINES, checkpoint_viewport, find_checkpoint, lookup_message_text,
        render_message_box,
    };
    use openjill_core::layout::LEVEL_MESSAGE_TICKS;
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{
        ActiveInput, ChangeLevelPayload, InputCommand, MessageDispatcher, MessagePayload,
        MessageType, RenderCommand, ScreenHandler, ScreenTransition,
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
    /// background layer still emits at least one `RenderCommand::Clear`.
    ///
    /// Preconditions: a synthetic JN buffer with zero objects and all-zero
    /// background map codes; the synthetic `AssetCache` carries an empty
    /// DMA, so every cell resolves to the transparent
    /// `StdBackgroundEntity` placeholder that emits no draw output.
    ///
    /// Invariants asserted: the tick completes without panic, and the
    /// resulting command list contains at least one `RenderCommand::Clear`
    /// so the renderer always has a baseline fill to execute even when both
    /// the object and background entity iterations contribute nothing.
    #[test]
    fn tick_with_no_objects_and_empty_backgrounds_emits_clear() {
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(
            result
                .commands
                .iter()
                .any(|cmd| matches!(cmd, RenderCommand::Clear { .. })),
            "tick must emit a Clear baseline command; got {:?}",
            result.commands
        );
    }

    /// Unit under test: `LevelScreen::tick` emits a sky `FillRect` over the
    /// game area immediately after the baseline `Clear` and before any
    /// background tile blits.
    ///
    /// Preconditions: synthetic level 1 screen built with
    /// [`EPISODE_1_SKY_COLOR`]; no objects and an empty background grid so
    /// no per-entity draws appear before the level base layer.
    ///
    /// Invariants asserted: a `FillRect` whose rectangle equals the
    /// `(GAME_AREA_X, GAME_AREA_Y, GAME_AREA_W, GAME_AREA_H)` window and whose
    /// `color` matches `EPISODE_1_SKY_COLOR` is emitted exactly once; its
    /// position in the command list is greater than the `Clear` baseline
    /// (so the sky paints over the clear) and less than any background
    /// `Blit` (so blits paint over the sky).
    #[test]
    fn tick_emits_sky_fill_rect_after_clear_and_before_blits() {
        use openjill_core::layout::{GAME_AREA_H, GAME_AREA_W, GAME_AREA_X, GAME_AREA_Y};
        let bytes = jn_bytes_with_objects(&[]);
        let (mut screen, _dispatcher) = screen_with_dispatcher(bytes, 1);
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());

        let clear_index = result
            .commands
            .iter()
            .position(|cmd| matches!(cmd, RenderCommand::Clear { .. }))
            .expect("tick must emit a Clear baseline command");
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

        assert!(
            sky_index > clear_index,
            "sky FillRect must follow the baseline Clear; commands: {:?}",
            result.commands
        );

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
    /// land at a higher index in the command list than the baseline
    /// `Clear` command, so a renderer executing in order paints the
    /// message box on top.
    #[test]
    fn tick_message_box_renders_after_base_frame() {
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
        let clear_index = commands
            .iter()
            .position(|cmd| matches!(cmd, RenderCommand::Clear { .. }))
            .expect("tick must emit a Clear baseline command");
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
            last_messagebox_blit > clear_index,
            "message-box overlay must follow the baseline Clear in the command list"
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
}
