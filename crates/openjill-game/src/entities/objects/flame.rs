//! Flame hazard entity (JN object type 31).
//!
//! Mirrors `org.jill.game.entities.obj.FlameManager` from the Java reference:
//! a stationary, lethal-on-touch decoration with an animated flame sprite.
//! The Rust port currently only implements the lethal-touch contract; sprite
//! animation and per-frame tile selection are deferred until the SHA tileset
//! identity for flame frames has been verified against the original
//! `JILL1.SHA` (see the SHA-verification note in
//! `docs/port/06-episode-1-gameplay.md`).

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Stationary lethal flame entity.
pub struct FlameEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl FlameEntity {
    /// Builds a flame from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
        }
    }
}

impl ObjectEntity for FlameEntity {
    /// No-op: flames are stationary.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Sprite rendering is deferred until the SHA tileset identity for the
    /// flame frames is verified; for now the entity emits no draw command.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Kills the player on touch by dispatching [`MessageType::DieRestartLevel`].
    ///
    /// The object trait does not expose a mutable reference to the player, so
    /// the death classification cannot be threaded into the player's
    /// [`ObjectEntity::on_kill`] from here.  The level loop will still
    /// restart the level on the next pending-request drain, which matches the
    /// issue scope's "dispatches DieRestartLevel" contract; tightening the
    /// classification path lands together with the rest of the enemy /
    /// hazard kill plumbing in epic 6 child issue 6.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
    }

    /// Flames are indestructible: weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the flame's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::FlameEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        MessageDispatcher, MessageHandler, MessagePayload, MessageType, ObjectEntity, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Object record byte length used by the synthetic JN buffer helper.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Test helper: records every delivered message into a shared buffer.
    struct RecordingHandler(Arc<Mutex<Vec<(MessageType, MessagePayload)>>>);

    impl MessageHandler for RecordingHandler {
        /// Appends `(msg_type, payload)` to the recording buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Builds a one-object JN file whose record carries `object_type = 31`
    /// (the flame type) and returns the inner `JnObject` clone.
    fn synthetic_flame_object() -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[count_off + 2] = 31; // object_type = FlameManager
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`FlameEntity::on_touch`].
    ///
    /// Preconditions: a flame is constructed from a synthetic JN object; a
    /// recording handler is subscribed to [`MessageType::DieRestartLevel`].
    ///
    /// Invariants asserted: `on_touch` dispatches exactly one
    /// `DieRestartLevel` message with the empty payload, matching the issue
    /// scope's lethal-touch contract.
    #[test]
    fn flame_on_touch_dispatches_die_restart_level() {
        let cache = AssetCache::synthetic();
        let mut flame = FlameEntity::new(&synthetic_flame_object(), &cache);
        let buffer: Arc<Mutex<Vec<(MessageType, MessagePayload)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::DieRestartLevel,
            Box::new(RecordingHandler(Arc::clone(&buffer))),
        );

        let state = RuntimeState::new();
        flame.on_touch(&state, &mut dispatcher);

        let received = buffer.lock().unwrap();
        assert_eq!(received.len(), 1, "exactly one DieRestartLevel dispatched");
        assert_eq!(
            received[0],
            (MessageType::DieRestartLevel, MessagePayload::None)
        );
    }
}
