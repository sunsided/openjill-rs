//! Switch entity (JN object type 32).
//!
//! Rust translation of `SwitchManager` from the Java reference
//! (`open-jill-object-background`).
//!
//! When the player's bounding box overlaps the switch, `on_touch` dispatches a
//! [`MessageType::Trigger`] message carrying the switch's link identifier
//! (stored in the JN `counter` field).  Any [`ToggleWallEntity`] with the same
//! link identifier will toggle its solid / passthrough state in response.
//!
//! Unlike pickup entities, the switch does not remove itself after activation;
//! the player can re-trigger it on every overlapping tick.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Switch entity.
pub struct SwitchEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Link identifier used to target the matching [`ToggleWallEntity`].
    ///
    /// Copied from the JN `counter` field; receivers compare this value
    /// against their own `counter` to decide whether the trigger applies.
    link_id: i32,
}

impl SwitchEntity {
    /// Builds a switch entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            link_id: i32::from(item.counter()),
        }
    }

    /// Returns the link identifier this switch broadcasts.
    pub fn link_id(&self) -> i32 {
        self.link_id
    }
}

impl ObjectEntity for SwitchEntity {
    /// Switches carry no per-tick logic.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Sprite rendering deferred pending SHA tileset verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Dispatches a `Trigger` message carrying the link identifier.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        dispatcher.send(MessageType::Trigger, MessagePayload::Count(self.link_id));
    }

    /// Switches are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the switch's bounding box for collision detection.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{MessageDispatcher, MessageHandler, MessagePayload, MessageType};
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Bytes needed for one JN object record.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a synthetic JN object record with `object_type = 32` and the
    /// supplied `counter` value (used as the link identifier).
    fn synthetic_switch(counter: i16) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 32;
        let counter_off = record_off + 19;
        bytes[counter_off..counter_off + 2].copy_from_slice(&counter.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Records each delivered message into a shared buffer.
    struct Recorder(Arc<Mutex<Vec<(MessageType, MessagePayload)>>>);

    impl MessageHandler for Recorder {
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Unit under test: `SwitchEntity::on_touch` dispatches a `Trigger`
    /// message carrying the configured link identifier.
    ///
    /// Preconditions: switch with `link_id = 7`; a recording handler is
    /// subscribed to `Trigger` messages.
    ///
    /// Invariants asserted: after `on_touch` exactly one `Trigger` message
    /// with payload `Count(7)` is delivered.
    #[test]
    fn on_touch_dispatches_trigger_with_link_id() {
        let cache = crate::asset_cache::AssetCache::synthetic();
        let item = synthetic_switch(7);
        let mut switch = SwitchEntity::new(&item, &cache);

        let buf: Arc<Mutex<Vec<(MessageType, MessagePayload)>>> = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(MessageType::Trigger, Box::new(Recorder(Arc::clone(&buf))));

        let state = RuntimeState::new();
        switch.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1, "exactly one Trigger message expected");
        assert_eq!(
            received[0],
            (MessageType::Trigger, MessagePayload::Count(7)),
            "Trigger must carry the switch link_id as Count payload"
        );
    }
}
