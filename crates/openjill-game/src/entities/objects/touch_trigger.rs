//! Touch trigger entity (JN object type 15).
//!
//! Mirrors `org.jill.game.entities.obj.TouchTriggerManager` from the Java
//! reference: when the player overlaps the trigger the entity dispatches a
//! [`MessageType::Trigger`] message carrying the trigger's `counter` field as
//! its link id.  Receivers (e.g. `ToggleWallEntity` from issue 63) inspect
//! the link id to decide whether the trigger targets them.
//!
//! Unlike pickup entities a touch trigger does not remove itself: the player
//! can pass over it repeatedly to re-fire the linked event, matching the
//! Java behaviour.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Touch trigger entity.
pub struct TouchTriggerEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Link identifier copied from the JN `counter` field; receivers compare
    /// this against their own `counter` to decide whether the trigger targets
    /// them.
    counter: i16,
}

impl TouchTriggerEntity {
    /// Builds a touch trigger from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: item.counter(),
        }
    }

    /// Returns the JN `counter` value used as the trigger's link identifier.
    pub fn counter(&self) -> i16 {
        self.counter
    }
}

impl ObjectEntity for TouchTriggerEntity {
    /// Touch triggers carry no per-tick state.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Triggers render no sprite.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Dispatches a [`MessageType::Trigger`] message carrying the trigger's
    /// link id.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        dispatcher.send(
            MessageType::Trigger,
            MessagePayload::Count(i32::from(self.counter)),
        );
    }

    /// Touch triggers are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the trigger's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}

#[cfg(test)]
mod tests {
    use super::TouchTriggerEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        MessageDispatcher, MessageHandler, MessagePayload, MessageType, ObjectEntity, RuntimeState,
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

    /// Builds a one-object JN file whose record has `object_type = 15` and
    /// the supplied `counter`, returning the inner `JnObject` clone.
    fn synthetic_trigger_object(counter: i16) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 15;
        let counter_off = record_off + 19;
        bytes[counter_off..counter_off + 2].copy_from_slice(&counter.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`TouchTriggerEntity::on_touch`].
    ///
    /// Invariants asserted: dispatches a [`MessageType::Trigger`] message
    /// carrying the trigger's `counter` as a [`MessagePayload::Count`] link
    /// id; touch triggers are not removed (subsequent touches re-fire).
    #[test]
    fn on_touch_dispatches_trigger_with_counter_link_id() {
        let cache = AssetCache::synthetic();
        let mut trigger = TouchTriggerEntity::new(&synthetic_trigger_object(7), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::Trigger,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        trigger.on_touch(&state, &mut dispatcher);
        trigger.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 2, "trigger re-fires on each touch");
        assert_eq!(
            received[0],
            (MessageType::Trigger, MessagePayload::Count(7))
        );
        assert!(
            !trigger.should_remove(),
            "touch triggers persist across hits"
        );
    }
}
