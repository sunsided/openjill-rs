//! Locked door entity (JN object type 24).
//!
//! Mirrors a simplified subset of `org.jill.game.entities.obj.LockedDoorManager`
//! from the Java reference.  The Java port resolves the required inventory
//! item from the background DMA name (`MAPDOOR` -> `GEM`, `DOORT` -> `RED_KEY`,
//! ...) and consumes the key by dispatching an `InventoryItem(item, false)`
//! message when a `TRIGGER` event matches the door's counter.
//!
//! Issue 60's acceptance contract is narrower: the door opens directly when
//! the player touches it while holding the matching key, and the touch is a
//! no-op otherwise.  The resolved key is set at construction from the JN
//! object's `info1` byte, which the Java port stores as the
//! `EnumInventoryObject` ordinal for the door's required inventory item; when
//! the JN data carries no usable hint the door falls back to
//! [`InventoryObject::Key`], the most common world-map case.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

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
    /// Inventory item the door requires to open.
    required_key: InventoryObject,
    /// `true` once the door has opened and should be removed.
    removed: bool,
}

impl LockedDoorEntity {
    /// Builds a locked door from a JN object record.
    ///
    /// Resolves the required key from the JN `info1` byte using the same
    /// `EnumInventoryObject` ordinal table used by [`BonusEntity`]; values
    /// outside the table fall back to [`InventoryObject::Key`].
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            required_key: resolve_door_key(item.info1()),
            removed: false,
        }
    }

    /// Returns the inventory item the door requires to open.
    pub fn required_key(&self) -> InventoryObject {
        self.required_key
    }
}

impl ObjectEntity for LockedDoorEntity {
    /// Doors carry no per-tick state.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Door sprite rendering is deferred until the DOORT / DOORB tile binding
    /// lands; the door is invisible to the renderer until then.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Opens the door when the player carries the matching key.
    ///
    /// On success this dispatches an `InventoryItem(required_key, false)`
    /// message to consume the carried key, then flags the door for removal.
    /// When the inventory does not contain the matching key the touch is a
    /// no-op (the player simply bumps into a closed door).
    fn on_touch(&mut self, state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        if !state.inventory.contains(&self.required_key) {
            return;
        }
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::remove(self.required_key)),
        );
        self.removed = true;
    }

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
}

/// Resolves a JN `info1` hint to the inventory item the door requires.
///
/// Mirrors the `EnumInventoryObject` ordinal mapping used by
/// [`crate::entities::objects::bonus::BonusEntity`]; values that the Rust
/// port has no first-class variant for fall back to [`InventoryObject::Key`],
/// which the Java reference uses for `DOORT` cells.
fn resolve_door_key(info1: i16) -> InventoryObject {
    match info1 {
        1 => InventoryObject::Key,
        2 => InventoryObject::Knife,
        3 => InventoryObject::Gem,
        8 => InventoryObject::Blade,
        _ => InventoryObject::Key,
    }
}

#[cfg(test)]
mod tests {
    use super::LockedDoorEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        InventoryItemPayload, InventoryObject, MessageDispatcher, MessageHandler, MessagePayload,
        MessageType, ObjectEntity, RuntimeState,
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
        /// Appends `(msg_type, payload)` to the recording buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Builds a one-object JN file whose record has `object_type = 24` and
    /// returns the inner `JnObject` clone.
    fn synthetic_door_object() -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[count_off + 2] = 24;
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`LockedDoorEntity::on_touch`] when the inventory
    /// contains the required key.
    ///
    /// Preconditions: `state.inventory` carries a [`InventoryObject::Key`]
    /// entry; a recording handler is subscribed to
    /// [`MessageType::InventoryItem`].
    ///
    /// Invariants asserted: `on_touch` dispatches exactly one
    /// `InventoryItem(Key, false)` message (the key consumption) and flags
    /// the door for removal.
    #[test]
    fn on_touch_opens_door_and_removes_key_when_present() {
        let cache = AssetCache::synthetic();
        let mut door = LockedDoorEntity::new(&synthetic_door_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::InventoryItem,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let mut state = RuntimeState::new();
        state.inventory.push(InventoryObject::Key);

        door.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(
            received.len(),
            1,
            "exactly one inventory message dispatched"
        );
        assert_eq!(
            received[0],
            (
                MessageType::InventoryItem,
                MessagePayload::InventoryItem(InventoryItemPayload::remove(InventoryObject::Key)),
            )
        );
        assert!(
            door.should_remove(),
            "door should be flagged for removal after opening"
        );
    }

    /// Unit under test: [`LockedDoorEntity::on_touch`] when the inventory
    /// does not contain the required key.
    ///
    /// Preconditions: `state.inventory` is empty; a recording handler is
    /// subscribed to [`MessageType::InventoryItem`].
    ///
    /// Invariants asserted: no messages dispatch and the door remains in
    /// place (`should_remove() == false`).
    #[test]
    fn on_touch_is_noop_when_key_absent() {
        let cache = AssetCache::synthetic();
        let mut door = LockedDoorEntity::new(&synthetic_door_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::InventoryItem,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        door.on_touch(&state, &mut dispatcher);

        assert!(
            buf.lock().unwrap().is_empty(),
            "no messages on closed touch"
        );
        assert!(
            !door.should_remove(),
            "door must remain when key not carried"
        );
    }
}
