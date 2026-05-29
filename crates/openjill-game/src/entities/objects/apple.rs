//! Apple pickup entity (JN object type 1).
//!
//! Mirrors `org.jill.game.entities.obj.AppleManager` from the Java reference:
//! when the player overlaps the apple it awards points and restores one health
//! segment (`point = 12`, `life = 1` in `object_conf.json`), then flags itself
//! for removal.  It is *not* an inventory item.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Points awarded for eating an apple (`AppleManager.point = 12`).
const APPLE_POINTS: i32 = 12;
/// Health segments restored (`AppleManager.life = 1`).
const APPLE_LIFE: i32 = 1;

/// SHA tileset index for apple sprites.
///
/// REVERSE-ENGINEERED: `AppleManager.tileSet = 9` in `object_conf.json`.
/// Tileset 9 is shared with other pickups (rock keys, gems); apple uses the
/// first four tiles. Not derivable from SHA structure; future engine config
/// file should expose this.
const TILESET: u8 = 9;

/// Base tile index within [`TILESET`].
///
/// REVERSE-ENGINEERED: `AppleManager.tile = 0` in `object_conf.json`.
const BASE_TILE: u16 = 0;

/// Number of animation frames cycled by the apple sprite.
///
/// REVERSE-ENGINEERED: `AppleManager.numberTileSet = 4` in `object_conf.json`.
/// Tileset 9 carries 8 tiles total (verified at construction by
/// [`AssetCache::assert_tile_subset`]); apple animates the first four.
const FRAME_COUNT: u16 = 4;

/// Apple pickup entity.
pub struct AppleEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Animation frame counter; incremented each tick, cycles every
    /// `FRAME_COUNT * 2` ticks so each tile is shown for 2 ticks.
    frame: u16,
    /// `true` once the apple has been touched by the player.
    removed: bool,
}

impl AppleEntity {
    /// Builds an apple from a JN object record.
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(TILESET, BASE_TILE + FRAME_COUNT, "AppleEntity FRAME_COUNT");
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            frame: 0,
            removed: false,
        }
    }
}

impl ObjectEntity for AppleEntity {
    /// Advances the animation frame counter.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Returns a `Blit` for the current animation frame.
    ///
    /// Cycles through `FRAME_COUNT` tiles (each shown for 2 ticks), matching
    /// `AppleManager.msgDraw` / `msgUpdate` from the Java reference.
    fn draw(&self) -> Option<RenderCommand> {
        let tile = BASE_TILE + (self.frame / 2) % FRAME_COUNT;
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Awards points, restores one health segment, and flags the apple for
    /// removal (Java `AppleManager.msgTouch`: `INVENTORY_POINT` + `INVENTORY_LIFE`).
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        dispatcher.send(
            MessageType::InventoryPoint,
            MessagePayload::Count(APPLE_POINTS),
        );
        dispatcher.send(
            MessageType::InventoryLife,
            MessagePayload::Count(APPLE_LIFE),
        );
        self.removed = true;
    }

    /// Apples are not damaged by weapons; this remains a no-op.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the apple's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the apple has been touched.
    fn should_remove(&self) -> bool {
        self.removed
    }
}

#[cfg(test)]
mod tests {
    use super::{APPLE_LIFE, APPLE_POINTS, AppleEntity};
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

    /// Test helper: records every delivered message into a shared buffer so
    /// the test can assert against what `on_touch` dispatched.
    struct RecordingHandler(RecordingBuffer);

    impl MessageHandler for RecordingHandler {
        /// Appends `(msg_type, payload)` to the recording buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Builds a one-object JN file whose record has `object_type = 1` and
    /// returns the inner `JnObject` clone.
    fn synthetic_apple_object() -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        bytes[count_off + 2] = 1; // object_type
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`AppleEntity::on_touch`].
    ///
    /// Preconditions: a synthetic apple is built from a zero-initialised JN
    /// object record; a recording handler is subscribed to
    /// [`MessageType::InventoryItem`] before `on_touch` runs.
    ///
    /// Invariants asserted: `on_touch` awards `APPLE_POINTS` and restores
    /// `APPLE_LIFE` health, and the entity reports `should_remove() == true`
    /// so the level loop drops it on the same tick.
    #[test]
    fn on_touch_awards_points_and_health_and_flags_for_removal() {
        let cache = AssetCache::synthetic();
        let mut apple = AppleEntity::new(&synthetic_apple_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::InventoryPoint,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );
        dispatcher.subscribe(
            MessageType::InventoryLife,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        apple.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 2, "points and life dispatched");
        assert!(received.contains(&(
            MessageType::InventoryPoint,
            MessagePayload::Count(APPLE_POINTS),
        )));
        assert!(received.contains(&(
            MessageType::InventoryLife,
            MessagePayload::Count(APPLE_LIFE),
        )));
        assert!(
            apple.should_remove(),
            "apple should flag itself for removal after pickup"
        );
    }

    /// Unit under test: [`AppleEntity::on_touch`] called twice does not
    /// re-dispatch the pickup messages.
    #[test]
    fn on_touch_is_idempotent_after_pickup() {
        let cache = AssetCache::synthetic();
        let mut apple = AppleEntity::new(&synthetic_apple_object(), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::InventoryPoint,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );
        dispatcher.subscribe(
            MessageType::InventoryLife,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        apple.on_touch(&state, &mut dispatcher);
        apple.on_touch(&state, &mut dispatcher);

        assert_eq!(buf.lock().unwrap().len(), 2, "second touch must be no-op");
    }
}
