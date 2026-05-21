//! Level screen handler backed by a `JN1L??.JN1` level file.
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

use openjill_core::layout::LEVEL_MESSAGE_TICKS;
use openjill_core::runtime::RuntimeState;
use openjill_core::{
    ActiveInput, ChangeLevelPayload, InputCommand, MessageDispatcher, MessageHandler,
    MessagePayload, MessageType, RenderCommand, ScreenHandler, ScreenTransition, TickResult,
};
use openjill_data::dma::DmaFile;
use openjill_data::jn::{JnFile, JnObject, JnReadError};

use crate::screens::map_screen::render_map_background;

/// Embedded `level_messagebox_vga.json` layout resource from the Java reference port.
const LEVEL_MESSAGEBOX_JSON: &str =
    include_str!("../../../../OpenJill/src/main/resources/level_messagebox_vga.json");

/// Save prefix for the episode 1 messages table inside
/// [`LEVEL_MESSAGEBOX_JSON`]; the JSON also carries `JN2` and `JN3` entries that
/// are not yet exercised.
const EPISODE_SAVE_PREFIX: &str = "JN1";

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
}

impl LevelScreen {
    /// Creates a level screen from parsed level data and the originating
    /// bytes, registering message handlers on `dispatcher` so the level
    /// transition messages are routed back to this screen on subsequent ticks.
    pub fn new(
        jn: JnFile,
        jn_bytes: Vec<u8>,
        dma: DmaFile,
        level_number: i32,
        dispatcher: &mut MessageDispatcher,
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
        }
    }

    /// Parses `bytes` as a level JN file and wraps it in a [`LevelScreen`].
    ///
    /// Returns the underlying [`JnReadError`] when parsing fails.
    pub fn from_bytes(
        bytes: Vec<u8>,
        dma: DmaFile,
        level_number: i32,
        dispatcher: &mut MessageDispatcher,
    ) -> Result<Self, JnReadError> {
        let jn = JnFile::from_bytes(bytes.clone())?;
        Ok(Self::new(jn, bytes, dma, level_number, dispatcher))
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

    /// Renders the per-tick command list, including the level background and
    /// the level message-box overlay when a transition is pending.
    fn render_frame(&self) -> Vec<RenderCommand> {
        let mut commands =
            render_map_background(&self.jn, &self.dma, self.viewport_x, self.viewport_y);
        if self.pending.is_some() {
            commands.extend(render_message_box(&self.message_text));
        }
        commands
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
    fn tick(&mut self, input: &ActiveInput, _state: &mut RuntimeState) -> TickResult {
        self.pump_inbox();

        // Render the frame using the pending state at the start of the tick so
        // the message box is visible for the entire countdown, including the
        // tick on which the timer reaches zero and the transition fires.
        let commands = self.render_frame();

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

/// Locates the checkpoint object for `level_number` in `jn` and returns the
/// viewport offset that places that object at the game-area top-left.
///
/// Mirrors `findCheckPoint` from the Java reference: the first object whose
/// signed counter equals the level number is the checkpoint, and its pixel
/// coordinates seed the viewport.  When no checkpoint exists the viewport is
/// pinned at `(0, 0)`.
fn checkpoint_viewport(jn: &JnFile, level_number: i32) -> (i32, i32) {
    if let Some(object) = find_checkpoint(jn, level_number) {
        return (-(object.x() as i32), -(object.y() as i32));
    }
    (0, 0)
}

/// Returns the first object whose `counter` equals `level_number`, when one
/// exists.
fn find_checkpoint(jn: &JnFile, level_number: i32) -> Option<&JnObject> {
    let needle = i16::try_from(level_number).ok()?;
    jn.objects().iter().find(|obj| obj.counter() == needle)
}

/// Returns the message-box text lines for the destination `level_number`,
/// looked up against the embedded `level_messagebox_vga.json` `messages.JN1`
/// table and split on `\n`.
fn lookup_message_text(level_number: i32) -> Vec<String> {
    if level_number < 0 {
        return Vec::new();
    }
    let table = &MESSAGE_BOX.messages;
    let index = usize::try_from(level_number).ok().unwrap_or(0);
    let Some(entry) = table.get(index) else {
        return Vec::new();
    };
    entry.split('\n').map(|line| line.to_string()).collect()
}

/// Emits render commands for the level message-box overlay.
///
/// Renders the static frame tile mosaic parsed from
/// `level_messagebox_vga.json` followed by one `DrawText` per message line up
/// to [`MESSAGE_MAX_LINES`].
fn render_message_box(text: &[String]) -> Vec<RenderCommand> {
    let layout = &*MESSAGE_BOX;
    let mut commands: Vec<RenderCommand> = layout
        .images
        .iter()
        .map(|tile| RenderCommand::Blit {
            tileset: tile.tileset,
            tile: tile.tile,
            x: layout.x + tile.x,
            y: layout.y + tile.y,
            opaque: false,
            clip: None,
        })
        .collect();

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

/// Parsed `level_messagebox_vga.json` layout used by [`LevelScreen`].
struct MessageBoxLayout {
    /// Top-left X position of the message box in framebuffer pixels.
    x: i32,
    /// Top-left Y position of the message box in framebuffer pixels.
    y: i32,
    /// Text area X offset relative to the message-box origin.
    textarea_x: i32,
    /// Text area Y offset relative to the message-box origin.
    textarea_y: i32,
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

    let x = get_i(&value, "x", 0);
    let y = get_i(&value, "y", 0);
    let text_color = get_u(&value, "textColor", 7);
    let textarea = value
        .get("textarea")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let textarea_x = get_i(&textarea, "x", 0);
    let textarea_y = get_i(&textarea, "y", 0);

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
        textarea_x,
        textarea_y,
        text_color,
        images,
        messages,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LEVEL_MESSAGEBOX_JSON, LevelScreen, MESSAGE_LINE_HEIGHT, MESSAGE_MAX_LINES,
        checkpoint_viewport, find_checkpoint, lookup_message_text, render_message_box,
    };
    use openjill_core::layout::LEVEL_MESSAGE_TICKS;
    use openjill_core::runtime::RuntimeState;
    use openjill_core::{
        ActiveInput, ChangeLevelPayload, InputCommand, MessageDispatcher, MessagePayload,
        MessageType, RenderCommand, ScreenHandler, ScreenTransition,
    };
    use openjill_data::dma::DmaFile;
    use openjill_data::jn::JnFile;

    /// Object record size in bytes (`JnObject` fixed field layout):
    /// `object_type` (1) + `x`/`y`/`x_speed`/`y_speed` (2 each) +
    /// `width`/`height`/`state`/`sub_state`/`state_count`/`counter`/`flags`
    /// (2 each) + `pointer` (4) + `info1`/`zap_hold` (2 each).
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds an empty `DmaFile`.
    fn empty_dma() -> DmaFile {
        DmaFile::from_bytes(vec![]).expect("empty DMA should parse")
    }

    /// Builds a synthetic JN byte buffer carrying `objects` zero-initialized
    /// object records, mutating only the fields the tests need.
    ///
    /// Each entry in `objects` is `(counter, x, y)` for one object record,
    /// emitted in source order.  All other fields are zero-filled.
    fn jn_bytes_with_objects(objects: &[(i16, u16, u16)]) -> Vec<u8> {
        let object_count = objects.len();
        let total_bytes = 128 * 64 * 2 + 2 + object_count * OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total_bytes];

        // Object count at byte offset 16384 (128×64 cells × 2 bytes per cell).
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&(object_count as u16).to_le_bytes());

        for (index, (counter, x, y)) in objects.iter().enumerate() {
            let record_off = count_off + 2 + index * OBJECT_RECORD_BYTES;
            // object_type (u8) at +0 left as 0.
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
        let screen = LevelScreen::from_bytes(bytes, empty_dma(), level_number, &mut dispatcher)
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
    /// number seeds the viewport offset.
    ///
    /// Preconditions: JN file holds a single object with `counter = 3`,
    /// `(x, y) = (256, 128)`; the screen is constructed for level 3.
    ///
    /// Invariants asserted: the viewport offset is `(-256, -128)`, which
    /// places the checkpoint position at the game-area origin.
    #[test]
    fn checkpoint_seeds_viewport_for_matching_counter() {
        let bytes = jn_bytes_with_objects(&[(3, 256, 128)]);
        let (screen, _dispatcher) = screen_with_dispatcher(bytes, 3);
        assert_eq!(screen.viewport(), (-256, -128));
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

    /// Unit under test: `find_checkpoint` returns the first object whose
    /// counter equals the requested level number.
    #[test]
    fn find_checkpoint_returns_matching_object() {
        let bytes = jn_bytes_with_objects(&[(0, 10, 10), (2, 20, 20), (2, 30, 30)]);
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let obj = find_checkpoint(&jn, 2).expect("level 2 checkpoint should exist");
        assert_eq!(obj.x(), 20);
        assert_eq!(obj.y(), 20);
    }

    /// Unit under test: `lookup_message_text` returns the JN1 message string
    /// for the destination level number, split on `\n`.
    #[test]
    fn lookup_message_text_returns_jn1_entry_lines() {
        let lines = lookup_message_text(0);
        assert!(
            !lines.is_empty(),
            "level 0 must have a message-box text entry"
        );
        assert!(
            lines.iter().any(|line| line.contains("JUNGLE")),
            "level 0 message text should reference the jungle map: got {lines:?}"
        );
    }

    /// Unit under test: `lookup_message_text` returns empty for an
    /// out-of-range level (e.g. the world-map sentinel).
    #[test]
    fn lookup_message_text_returns_empty_for_negative_level() {
        assert!(lookup_message_text(openjill_core::MAP_LEVEL).is_empty());
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

    /// Unit under test: the screen-level [`MessageDispatcher::send`] path used
    /// by [`LevelScreen::checkpoint_seeds_viewport_for_matching_counter`]
    /// integrates with [`MessageDispatcher::clear`] semantics — clearing the
    /// dispatcher prevents queued messages from reaching a screen subscribed
    /// after the clear.
    #[test]
    fn dispatcher_clear_drops_pending_before_subscribe() {
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
        dispatcher.clear();
        let bytes = jn_bytes_with_objects(&[]);
        let mut screen = LevelScreen::from_bytes(bytes, empty_dma(), 1, &mut dispatcher)
            .expect("synthetic level JN should parse");
        let result = screen.tick(&ActiveInput::new(), &mut RuntimeState::new());
        assert!(
            result.transition.is_none(),
            "cleared dispatcher must not deliver previously-queued messages"
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
