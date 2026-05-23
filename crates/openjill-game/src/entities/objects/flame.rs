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
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
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
    /// Pending player-kill classification armed in [`Self::on_touch`] and
    /// drained by the level loop via [`ObjectEntity::take_player_kill`] so the
    /// kill can be applied to the player's [`ObjectEntity::on_kill`].
    pending_kill: Option<DeathKind>,
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
            pending_kill: None,
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

    /// Arms a [`DeathKind::OtherBackground`] kill on the player.  The level
    /// loop drains this through [`Self::take_player_kill`] and applies it via
    /// `player.on_kill`, so the player enters its `Die` sub-state and the
    /// `DieRestartLevel` dispatch is left to the player after the die
    /// animation finishes.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        self.pending_kill = Some(DeathKind::OtherBackground);
    }

    /// Flames are indestructible: weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the flame's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns the pending kill classification (and clears it) so the level
    /// loop can apply it to the player after the touch dispatch pass.
    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::FlameEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        DeathKind, MessageDispatcher, MessageHandler, MessagePayload, MessageType, ObjectEntity,
        RuntimeState,
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

    /// Unit under test: [`FlameEntity::on_touch`] + [`FlameEntity::take_player_kill`].
    ///
    /// Preconditions: a flame is constructed from a synthetic JN object; a
    /// recording handler is subscribed to [`MessageType::DieRestartLevel`].
    ///
    /// Invariants asserted: `on_touch` arms a [`DeathKind::OtherBackground`]
    /// kill on the flame (drained via `take_player_kill`) but does **not**
    /// dispatch `DieRestartLevel` directly; the level loop is responsible for
    /// applying the kill to the player whose `Die` sub-state then drives
    /// `DieRestartLevel` after the die animation completes.  A second
    /// `take_player_kill` returns `None`, confirming the slot is drained.
    #[test]
    fn flame_on_touch_arms_player_kill_without_dispatching_restart() {
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

        assert!(
            buffer.lock().unwrap().is_empty(),
            "on_touch must not dispatch DieRestartLevel directly"
        );
        assert_eq!(flame.take_player_kill(), Some(DeathKind::OtherBackground));
        assert_eq!(
            flame.take_player_kill(),
            None,
            "pending kill must be drained after the first take"
        );
    }
}
