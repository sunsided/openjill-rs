//! Lift (moving platform) entity (JN object type 61).
//!
//! Rust translation of `LiftManager` from the Java reference
//! (`open-jill-object-background`).
//!
//! A lift moves horizontally or vertically at the speed stored in the JN
//! `x_speed`/`y_speed` fields.  On each tick, when the player's bounding
//! box overlaps the lift's bounding box, the lift dispatches a
//! [`MessageType::PlayerMove`] message carrying the per-tick delta so the
//! player rides the platform.  The actual player-side subscription to
//! `PlayerMove` is wired during level initialisation by the level screen.
//!
//! Collision with map walls is not yet implemented; the lift moves freely
//! until a patrol-range implementation is added in a future issue.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Lift (moving platform) entity.
pub struct LiftEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick.
    xd: i32,
    /// Vertical velocity in pixels per tick.
    yd: i32,
    /// `true` when the player's bounding box overlapped this lift during the
    /// most recent [`ObjectEntity::observe_player`] call.
    player_on_lift: bool,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position and velocity written
    /// back.
    origin: JnObject,
}

impl LiftEntity {
    /// Builds a lift entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            xd: i32::from(item.x_speed()),
            yd: i32::from(item.y_speed()),
            player_on_lift: false,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for LiftEntity {
    /// Advances the lift position and dispatches `PlayerMove` when the player
    /// is riding.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.player_on_lift {
            dispatcher.send(
                MessageType::PlayerMove,
                MessagePayload::Move(self.xd, self.yd),
            );
        }
        self.x += self.xd;
        self.y += self.yd;
    }

    /// Sprite rendering deferred pending SHA tileset verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Lifts do not react to direct player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Lifts are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the lift's bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Snapshots the live lift for a save game (always persisted).
    ///
    /// Persists position and velocity so the moving platform resumes its path
    /// from where it was; the `player_on_lift` flag re-derives on the next tick.
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        obj.set_speed(self.xd as i16, self.yd as i16);
        Some(obj)
    }

    /// Records whether the player is standing on the lift.
    ///
    /// Called by the level loop once per tick before `update`.  The lift
    /// treats any bounding-box overlap as "player on lift" and broadcasts
    /// `PlayerMove` accordingly on the subsequent `update` call.
    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_on_lift = player_bbox.intersects(&self.bounding_box());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundEntity,
        BackgroundGrid, MessageDispatcher, MessageHandler, MessagePayload, MessageType,
        ObjectEntity, Rect, RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Bytes needed for one JN object record.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a synthetic lift JN object at `(x, y)` with the supplied speeds.
    fn synthetic_lift(x: u16, y: u16, x_speed: i16, y_speed: i16) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let rec = count_off + 2;
        bytes[rec] = 61;
        bytes[rec + 1..rec + 3].copy_from_slice(&x.to_le_bytes());
        bytes[rec + 3..rec + 5].copy_from_slice(&y.to_le_bytes());
        bytes[rec + 5..rec + 7].copy_from_slice(&x_speed.to_le_bytes());
        bytes[rec + 7..rec + 9].copy_from_slice(&y_speed.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Inert background cell for the required grid parameter.
    struct EmptyBg;
    impl BackgroundEntity for EmptyBg {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            true
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds an empty 128x64 background grid.
    fn empty_grid() -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> =
            Vec::with_capacity(BACKGROUND_GRID_HEIGHT);
        for _ in 0..BACKGROUND_GRID_HEIGHT {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(BACKGROUND_GRID_WIDTH);
            for _ in 0..BACKGROUND_GRID_WIDTH {
                row.push(Box::new(EmptyBg));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Records each delivered message into a shared buffer.
    struct Recorder(Arc<Mutex<Vec<(MessageType, MessagePayload)>>>);

    impl MessageHandler for Recorder {
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Unit under test: `LiftEntity` dispatches `PlayerMove` each tick while
    /// the player's bounding box intersects the lift's bounding box.
    ///
    /// Preconditions: lift at `(64, 64)` with `xd = 2`, `yd = 0`; player
    /// bounding box overlapping the lift.
    ///
    /// Invariants asserted: after one tick with an overlapping player bbox,
    /// exactly one `PlayerMove(Move(2, 0))` message is delivered.
    #[test]
    fn lift_dispatches_player_move_when_player_overlaps() {
        let cache = crate::asset_cache::AssetCache::synthetic();
        let item = synthetic_lift(64, 64, 2, 0);
        let mut lift = LiftEntity::new(&item, &cache);

        let player_bbox = Rect::new(64, 64, 16, 16);
        lift.observe_player(player_bbox);

        let buf: Arc<Mutex<Vec<(MessageType, MessagePayload)>>> = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::PlayerMove,
            Box::new(Recorder(Arc::clone(&buf))),
        );

        let input = ActiveInput::new();
        let state = RuntimeState::new();
        let grid = empty_grid();
        lift.update(&input, &state, &grid, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1, "exactly one PlayerMove message expected");
        assert_eq!(
            received[0],
            (MessageType::PlayerMove, MessagePayload::Move(2, 0)),
            "PlayerMove must carry (xd, yd) as Move payload"
        );
    }

    /// Unit under test: `LiftEntity` does NOT dispatch `PlayerMove` when the
    /// player is not on the lift.
    ///
    /// Preconditions: lift at `(64, 64)`; player bbox far away at `(200, 200)`.
    ///
    /// Invariants asserted: no `PlayerMove` message is dispatched.
    #[test]
    fn lift_does_not_dispatch_when_player_absent() {
        let cache = crate::asset_cache::AssetCache::synthetic();
        let item = synthetic_lift(64, 64, 2, 0);
        let mut lift = LiftEntity::new(&item, &cache);

        lift.observe_player(Rect::new(200, 200, 16, 16));

        let buf: Arc<Mutex<Vec<(MessageType, MessagePayload)>>> = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::PlayerMove,
            Box::new(Recorder(Arc::clone(&buf))),
        );

        let input = ActiveInput::new();
        let state = RuntimeState::new();
        let grid = empty_grid();
        lift.update(&input, &state, &grid, &mut dispatcher);

        assert!(
            buf.lock().unwrap().is_empty(),
            "no PlayerMove must be dispatched when player is not on the lift"
        );
    }
}
