//! Flame hazard entity (JN object type 31).
//!
//! Mirrors `org.jill.game.entities.obj.FlameManager` from the Java reference:
//! a lethal-on-touch animated flame with a fixed-length frame cycle that
//! self-kills once the animation completes (the Java reference's `killMe()`
//! after `counter >= images.length`).
//!
//! Tile / tileset identity comes from `object_conf.json`:
//! `tileSet = 12`, `tile = 0`, `numberTileSet = 6`.  The Java loader builds a
//! 12-slot frame array where every source tile is repeated twice
//! (`images[0] = images[1] = tile 0`, `images[2] = images[3] = tile 1`, ...),
//! so the animation runs at half the tick rate.  The Rust port preserves the
//! same counter convention.
//!
//! FIXME(epic-6): the `(tileset, tile)` pair is taken from the Java config
//! verbatim; once the SHA dumper grows a visual / PNG output, confirm tileset
//! 12 tiles 0..=5 are the expected flame frames against the original
//! `JILL1.SHA`.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset index that owns every flame frame.
///
/// REVERSE-ENGINEERED: `FlameManager.tileSet = 12` in `object_conf.json`.
/// Tileset 12 carries 12 tiles total; flame animates the first 6. Future
/// engine config file should expose this.
const TILESET_INDEX: u8 = 12;

/// Base tile index inside [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED: `FlameManager.tile = 0` in `object_conf.json`.
const TILE_BASE: u16 = 0;

/// Number of distinct flame source tiles.
///
/// REVERSE-ENGINEERED: `FlameManager.numberTileSet = 6` in
/// `object_conf.json`. Verified at construction by
/// [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: i32 = 6;

/// Total animation length in ticks; each source tile is held for two ticks so
/// the frame count is `NUMBER_TILE_SET * 2` (mirrors the Java
/// `imagesUp = new Optional[numberTileSet * 2]` slot count).
///
/// REVERSE-ENGINEERED: derived from `NUMBER_TILE_SET`.
const FRAME_COUNT: i32 = NUMBER_TILE_SET * 2;

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
    /// Animation counter incremented each tick.  Mirrors the Java
    /// `FlameManager.counter` field; the rendered tile is `TILE_BASE +
    /// counter / 2` so each source tile is held for two ticks.
    counter: i32,
    /// `true` once the animation completes and the entity has signalled the
    /// level loop to remove it from the active object list (the Java
    /// reference's `killMe()` call).
    removed: bool,
    /// Pending player-kill classification armed in [`Self::on_touch`] and
    /// drained by the level loop via [`ObjectEntity::take_player_kill`] so the
    /// kill can be applied to the player's [`ObjectEntity::on_kill`].
    pending_kill: Option<DeathKind>,
}

impl FlameEntity {
    /// Builds a flame from a JN object record.
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET as u16,
            "FlameEntity NUMBER_TILE_SET",
        );
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: 0,
            removed: false,
            pending_kill: None,
        }
    }

    /// Returns the source tile index for the current `counter` value.
    ///
    /// `counter / 2` selects the source tile so each frame is held for two
    /// ticks, matching the Java reference's doubled `imagesUp` layout.
    fn current_tile(&self) -> u16 {
        let frame = (self.counter / 2).clamp(0, NUMBER_TILE_SET - 1) as u16;
        TILE_BASE + frame
    }
}

impl ObjectEntity for FlameEntity {
    /// Advances the flame's animation counter and signals removal once the
    /// frame cycle completes (`counter >= FRAME_COUNT`, the Java reference's
    /// `counter >= img.length` check).  Mirrors `FlameManager.msgUpdate`.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        if self.removed {
            return;
        }
        self.counter += 1;
        if self.counter >= FRAME_COUNT {
            // Mirror the Java reference's `counter--; killMe()` sequence so
            // the counter value is consistent, even though the Rust port
            // suppresses drawing once `removed` is set.
            self.counter -= 1;
            self.removed = true;
        }
    }

    /// Emits a [`RenderCommand::Blit`] for the current flame frame.
    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: self.current_tile(),
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Arms a [`DeathKind::OtherBackground`] kill on the player.  The level
    /// loop drains this through [`Self::take_player_kill`] and applies it via
    /// `player.on_kill`, so the player enters its `Die` sub-state and the
    /// `DieRestartLevel` dispatch is left to the player after the die
    /// animation finishes.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        self.pending_kill = Some(DeathKind::OtherBackground);
    }

    /// Flames are indestructible: weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the flame's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the flame's animation has completed and the level
    /// loop should drop it from the active object list.
    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Returns the pending kill classification (and clears it) so the level
    /// loop can apply it to the player after the touch dispatch pass.
    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::{FRAME_COUNT, FlameEntity, NUMBER_TILE_SET, TILE_BASE, TILESET_INDEX};
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessageHandler, MessagePayload,
        MessageType, ObjectEntity, RenderCommand, RuntimeState,
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

    /// Unit under test: [`FlameEntity::draw`] emits a `Blit` against the
    /// SHA-verified `(tileset, tile)` pair for the first frame.
    ///
    /// Preconditions: a flame is constructed from a synthetic JN object; no
    /// ticks have run yet so `counter = 0`.
    ///
    /// Invariants asserted: the draw command targets [`TILESET_INDEX`] /
    /// [`TILE_BASE`] (the first source tile), and the destination pixel
    /// coordinates equal the flame's world origin.
    #[test]
    fn flame_draw_emits_first_frame_blit() {
        let cache = AssetCache::synthetic();
        let flame = FlameEntity::new(&synthetic_flame_object(), &cache);

        let cmd = flame.draw().expect("active flame must emit a blit");
        match cmd {
            RenderCommand::Blit { tileset, tile, .. } => {
                assert_eq!(tileset, TILESET_INDEX, "flame must blit from tileset 12");
                assert_eq!(tile, TILE_BASE, "first frame must use the base tile");
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }

    /// Unit under test: [`FlameEntity::update`] cycles through the doubled
    /// frame array and then signals removal once the animation completes.
    ///
    /// Preconditions: a flame is constructed; we tick it `FRAME_COUNT` times.
    ///
    /// Invariants asserted: after [`FRAME_COUNT`] ticks the flame reports
    /// `should_remove() == true`; the tile index advanced past the first
    /// source tile (so the animation actually played); subsequent draws are
    /// suppressed once removed.
    #[test]
    fn flame_update_advances_then_marks_removal() {
        let cache = AssetCache::synthetic();
        let mut flame = FlameEntity::new(&synthetic_flame_object(), &cache);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let backgrounds = BackgroundGrid::new(Vec::new());
        let mut dispatcher = MessageDispatcher::new();

        // Tick through to half the animation and confirm a later frame is
        // selected (`counter = 4` -> tile `TILE_BASE + 2`).
        for _ in 0..4 {
            flame.update(&input, &state, &backgrounds, &mut dispatcher);
        }
        match flame.draw().expect("mid-animation flame still draws") {
            RenderCommand::Blit { tile, .. } => assert_eq!(tile, TILE_BASE + 2),
            other => panic!("expected Blit mid-animation, got {other:?}"),
        }

        // Drive the remaining ticks past `FRAME_COUNT`; the entity must mark
        // itself for removal exactly once the counter would cross the bound.
        for _ in 4..FRAME_COUNT {
            flame.update(&input, &state, &backgrounds, &mut dispatcher);
        }
        assert!(
            flame.should_remove(),
            "flame must request removal once the {FRAME_COUNT}-tick animation completes"
        );
        assert!(
            flame.draw().is_none(),
            "removed flame must stop emitting blits"
        );
        let _ = NUMBER_TILE_SET; // keep the import live in case future tests add bounds checks.
    }
}
