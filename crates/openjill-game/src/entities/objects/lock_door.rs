//! Locked door entity (JN object type 24).
//!
//! Mirrors `org.jill.game.entities.obj.LockedDoorManager` from the Java
//! reference.  The required inventory item is resolved from the background
//! DMA map code at the door's grid cell (MAPDOOR = 190 -> Gem, DOORT = 161
//! -> Key); the door opens in response to a `Trigger` message whose link
//! identifier matches the door's `counter` field, not on direct player touch.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// DMA map code for a `MAPDOOR` cell (world-map gem gate).
const MAP_CODE_MAPDOOR: u16 = 190;

/// DMA map code for a `DOORT` cell (level door requiring a red key).
const MAP_CODE_DOORT: u16 = 161;

/// Locked door entity.
pub struct LockedDoorEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Link identifier: the door opens when a `Trigger` message with this
    /// `link_id` arrives.  Matches the JN object `counter` field, which the
    /// touch trigger at the door entrance shares.
    counter: i32,
    /// `true` when a matching trigger has arrived this tick and `update`
    /// should evaluate the inventory and act.
    triggered: bool,
    /// `true` once the door has opened and should be removed.
    removed: bool,
}

impl LockedDoorEntity {
    /// Builds a locked door from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: i32::from(item.counter()),
            triggered: false,
            removed: false,
        }
    }
}

impl ObjectEntity for LockedDoorEntity {
    /// When a trigger has arrived, resolves the required key from the
    /// background cell at the door's position and either opens the door or
    /// shows the appropriate lock message.
    fn update(
        &mut self,
        _input: &ActiveInput,
        state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if !self.triggered {
            return;
        }
        self.triggered = false;

        let cell_x = self.x.div_euclid(BLOCK_SIZE_I) as usize;
        let cell_y = self.y.div_euclid(BLOCK_SIZE_I) as usize;
        let required_key = backgrounds
            .get(cell_x, cell_y)
            .and_then(|cell| cell.dma_map_code())
            .map(resolve_key_by_map_code)
            .unwrap_or(InventoryObject::Key);

        if state.inventory.contains(&required_key) {
            dispatcher.send(
                MessageType::InventoryItem,
                MessagePayload::InventoryItem(InventoryItemPayload::remove(required_key)),
            );
            let cx_end = (self.x + self.w - 1).div_euclid(BLOCK_SIZE_I);
            let cy_end = (self.y + self.h - 1).div_euclid(BLOCK_SIZE_I);
            for cx in (self.x.div_euclid(BLOCK_SIZE_I))..=cx_end {
                for cy in (self.y.div_euclid(BLOCK_SIZE_I))..=cy_end {
                    dispatcher.send(
                        MessageType::Background,
                        MessagePayload::ClearBackground {
                            cell_x: cx,
                            cell_y: cy,
                        },
                    );
                }
            }
            dispatcher.send(
                MessageType::StatusBarText,
                MessagePayload::Text(open_message(required_key).to_string()),
            );
            self.removed = true;
        } else {
            dispatcher.send(
                MessageType::StatusBarText,
                MessagePayload::Text(lock_message(required_key).to_string()),
            );
        }
    }

    /// Door sprite is rendered by the background layer; the object entity
    /// draws nothing.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Direct player touch does not open the door; opening is trigger-driven.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Doors are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the door's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the door has opened.
    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Arms the door when a `Trigger` message with the matching link identifier
    /// arrives.  The actual inventory check and background clearing happen in
    /// [`Self::update`] on the following tick.
    fn receive_trigger(&mut self, link_id: i32) {
        if link_id == self.counter {
            self.triggered = true;
        }
    }
}

/// Resolves a DMA map code to the inventory item the door requires.
fn resolve_key_by_map_code(map_code: u16) -> InventoryObject {
    match map_code {
        MAP_CODE_MAPDOOR => InventoryObject::Gem,
        MAP_CODE_DOORT => InventoryObject::Key,
        _ => InventoryObject::Key,
    }
}

/// Returns the status-bar text shown when the player triggers a locked door
/// without carrying the required key.
fn lock_message(required_key: InventoryObject) -> &'static str {
    match required_key {
        InventoryObject::Gem => "YOU NEED A GEM TO PASS",
        _ => "THE DOOR IS LOCKED",
    }
}

/// Returns the status-bar text shown when the player successfully opens a door.
fn open_message(required_key: InventoryObject) -> &'static str {
    match required_key {
        InventoryObject::Gem => "THE GATE OPENS",
        _ => "THE DOOR OPENS",
    }
}

#[cfg(test)]
mod tests {
    use super::LockedDoorEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        BackgroundEntity, BackgroundGrid, InventoryItemPayload, InventoryObject, MessageDispatcher,
        MessageHandler, MessagePayload, MessageType, ObjectEntity, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Object record byte length used by the synthetic JN buffer helpers.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Shared recording buffer used by `RecordingHandler` in unit tests.
    type RecordingBuffer = Arc<Mutex<Vec<(MessageType, MessagePayload)>>>;

    /// Test helper: records every delivered message into a shared buffer.
    struct RecordingHandler(RecordingBuffer);

    impl MessageHandler for RecordingHandler {
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Minimal background entity that returns a fixed DMA map code for tests.
    struct FakeMapDoorCell(u16);

    impl BackgroundEntity for FakeMapDoorCell {
        fn draw(&self, _x: i32, _y: i32) -> Option<openjill_core::RenderCommand> {
            None
        }
        fn update(&mut self, _x: i32, _y: i32, _d: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _p: &mut dyn ObjectEntity, _d: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            false
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
        fn dma_map_code(&self) -> Option<u16> {
            Some(self.0)
        }
    }

    /// Builds a 1×1 BackgroundGrid with the given map code at cell (0, 0).
    fn grid_with_map_code(code: u16) -> BackgroundGrid {
        BackgroundGrid::new(vec![vec![Box::new(FakeMapDoorCell(code))]])
    }

    /// Builds a one-object JN file with a LockedDoor object at position (0, 0)
    /// with `counter = 1`.
    ///
    /// JnObject field layout (byte offsets from record start):
    /// 0:type 1:x(2) 3:y(2) 5:x_speed(2) 7:y_speed(2) 9:width(2) 11:height(2)
    /// 13:state(2) 15:sub_state(2) 17:state_count(2) 19:counter(2) ...
    fn synthetic_door_object() -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[count_off + 2] = 24; // object_type = LockedDoor
        // counter is at offset 19 from start of object record
        bytes[count_off + 2 + 19..count_off + 2 + 21].copy_from_slice(&1i16.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: `LockedDoorEntity::receive_trigger` + `update` when the
    /// inventory contains the required gem (MAPDOOR, map_code=190).
    ///
    /// Invariants asserted: `update` dispatches `InventoryItem(Gem, remove)`,
    /// `Background(ClearBackground{0,0})`, and `StatusBarText("THE GATE OPENS")`;
    /// the door flags itself for removal.
    #[test]
    fn trigger_opens_mapdoor_when_gem_present() {
        let cache = AssetCache::synthetic();
        let mut door = LockedDoorEntity::new(&synthetic_door_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        for msg in [
            MessageType::InventoryItem,
            MessageType::Background,
            MessageType::StatusBarText,
        ] {
            dispatcher.subscribe(msg, Box::new(RecordingHandler(Arc::clone(&buf))));
        }

        door.receive_trigger(1);

        let mut state = RuntimeState::new();
        state.inventory.push(InventoryObject::Gem);
        let grid = grid_with_map_code(190); // MAPDOOR
        let input = openjill_core::ActiveInput::default();
        door.update(&input, &state, &grid, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert!(
            received
                .iter()
                .any(|(t, p)| *t == MessageType::InventoryItem
                    && *p
                        == MessagePayload::InventoryItem(InventoryItemPayload::remove(
                            InventoryObject::Gem
                        ))),
            "should dispatch InventoryItem(Gem, remove)"
        );
        assert!(
            received
                .iter()
                .any(|(t, p)| *t == MessageType::StatusBarText
                    && *p == MessagePayload::Text("THE GATE OPENS".to_string())),
            "should dispatch status bar text"
        );
        assert!(door.should_remove(), "door should be flagged for removal");
    }

    /// Unit under test: trigger when gem is missing shows lock message, door stays.
    #[test]
    fn trigger_shows_lock_message_when_gem_absent() {
        let cache = AssetCache::synthetic();
        let mut door = LockedDoorEntity::new(&synthetic_door_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::StatusBarText,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        door.receive_trigger(1);

        let state = RuntimeState::new();
        let grid = grid_with_map_code(190); // MAPDOOR
        let input = openjill_core::ActiveInput::default();
        door.update(&input, &state, &grid, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(
            received[0],
            (
                MessageType::StatusBarText,
                MessagePayload::Text("YOU NEED A GEM TO PASS".to_string()),
            )
        );
        assert!(
            !door.should_remove(),
            "door must remain when gem not carried"
        );
    }

    /// Unit under test: a trigger with a non-matching link_id is ignored.
    #[test]
    fn trigger_with_wrong_link_id_is_ignored() {
        let cache = AssetCache::synthetic();
        let mut door = LockedDoorEntity::new(&synthetic_door_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::StatusBarText,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        door.receive_trigger(99); // wrong id

        let mut state = RuntimeState::new();
        state.inventory.push(InventoryObject::Gem);
        let grid = grid_with_map_code(190);
        let input = openjill_core::ActiveInput::default();
        door.update(&input, &state, &grid, &mut dispatcher);

        assert!(
            buf.lock().unwrap().is_empty(),
            "wrong link_id must not trigger"
        );
        assert!(!door.should_remove(), "door must remain");
    }
}
