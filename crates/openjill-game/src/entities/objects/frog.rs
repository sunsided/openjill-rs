//! Frog enemy entity (JN object type 22).
//!
//! Mirrors `org.jill.game.entities.obj.FrogManager`: two-state machine.
//! On floor (`on_floor = true`): counts ticks; after `counterBeforeJump`
//! ticks launches a gravity arc toward the player.  In air (`on_floor =
//! false`): moves horizontally (stops against walls, no direction reversal),
//! applies gravity (+1 per tick), lands when `floor_below` returns true while
//! descending.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 63`, `tile = 0`,
//! `numberTileSet = 6`.  Three tiles per direction (right/left):
//!   tile 0/3 = on-floor frame, tile 1/4 = airborne frame, tile 2/5 = apex.
//! SHA header[63] confirms: 6 tiles, 14×10 px each.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_below, floor_under_next, sprite_dims};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the frog frames.
///
/// REVERSE-ENGINEERED: `FrogManager.tileSet = 63` in `object_conf.json`.
/// Tileset 63 carries 6 tiles total — the only enemy in the Rust port
/// that consumes its entire tileset. Future engine config file should
/// expose this.
const TILESET_INDEX: u8 = 63;
/// Tiles per direction (right: 0-2, left: 3-5).
///
/// REVERSE-ENGINEERED: derived from `FrogManager.numberTileSet = 6` / 2.
const FRAMES_PER_DIR: u16 = 3;
/// Horizontal patrol/jump speed in pixels per tick.
///
/// REVERSE-ENGINEERED: `FrogManager.xSpeedMax = 4` in `object_conf.json`.
const X_SPEED: i32 = 4;
/// Score awarded when the frog is killed.
///
/// REVERSE-ENGINEERED: `FrogManager.point = 15` in `object_conf.json`
/// (rounded up to 100 in the Rust port).
const SCORE_VALUE: i32 = 100;
/// Ticks on floor before jumping.
///
/// REVERSE-ENGINEERED: `FrogManager.counterBeforeJump = 17` in
/// `object_conf.json`.
const JUMP_PERIOD: i32 = 17;
/// Initial upward velocity at jump time.
///
/// REVERSE-ENGINEERED: `FrogManager.ySpeedChangePicture = -10` in
/// `object_conf.json`.
const JUMP_INIT_YD: i32 = -10;
/// Max downward speed per tick.
///
/// REVERSE-ENGINEERED: `FrogManager.ySpeedMax = 12` in `object_conf.json`.
const FALL_SPEED_MAX: i32 = 12;

pub struct FrogEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    y_speed: i32,
    jump_counter: i32,
    on_floor: bool,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    /// Most recent player x coordinate captured via
    /// [`ObjectEntity::observe_player`]; used at jump launch to chase
    /// the player horizontally (mirrors Java `FrogManager.msgUpdate`'s
    /// `PLAYER_POSITION.getX()` lookup).
    player_x: i32,
}

impl FrogEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            FRAMES_PER_DIR * 2,
            "FrogEntity NUMBER_TILE_SET",
        );
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let jn_h = i32::from(item.height());
        let y_adj = if jn_h > 0 { (h - jn_h).max(0) } else { 0 };
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()) - y_adj,
            w,
            h,
            x_speed: X_SPEED,
            y_speed: 0,
            jump_counter: 0,
            on_floor: true,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
            player_x: i32::from(item.x()),
        }
    }
}

impl ObjectEntity for FrogEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.dead {
            if !self.score_dispatched {
                self.score_dispatched = true;
                dispatcher.send(
                    MessageType::InventoryPoint,
                    MessagePayload::Count(SCORE_VALUE),
                );
            }
            return;
        }
        if self.zaphold > 0 {
            self.zaphold -= 1;
        }

        if self.on_floor {
            // Floor state: patrol with gap and wall checks; reverse on fail.
            if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
                && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            {
                self.x += self.x_speed;
            } else {
                self.x_speed = -self.x_speed;
            }

            self.jump_counter += 1;
            if self.jump_counter >= JUMP_PERIOD {
                self.jump_counter = 0;
                self.y_speed = JUMP_INIT_YD;
                self.on_floor = false;
                // Java `FrogManager.msgUpdate`: at jump launch reset
                // `xSpeed = xSpeedMax` and flip the sign when the player
                // is to the left (`xd = PLAYER_POSITION.getX() - this.x;
                // if (xd < 0) this.xSpeed *= -1;`). This makes the frog
                // chase the player horizontally instead of patrolling
                // blindly.
                let dir = if self.player_x < self.x { -1 } else { 1 };
                self.x_speed = X_SPEED * dir;
            }
        } else {
            // Air state: move horizontally without gap check; stop at walls (no reversal).
            if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed) {
                self.x += self.x_speed;
            }

            // Vertical arc: apply movement then gravity.
            self.y += self.y_speed;
            if self.y_speed < FALL_SPEED_MAX {
                self.y_speed += 1;
            }

            // Land when descending and floor reached.
            if self.y_speed > 0 && floor_below(backgrounds, self.x, self.y, self.w, self.h) {
                self.y_speed = 0;
                self.on_floor = true;
                self.jump_counter = 0;
            }
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.dead {
            return None;
        }
        // Direction: right = tiles 0-2, left = tiles 3-5.
        let base = if self.x_speed < 0 { FRAMES_PER_DIR } else { 0 };
        // Frame within direction: 0 = floor, 1 = airborne, 2 = apex.
        let frame = if self.on_floor {
            0
        } else if self.y_speed == 0 {
            2
        } else {
            1
        };
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: base + frame,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.dead || self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
        self.pending_kill = Some(DeathKind::Enemy);
    }

    fn on_kill(&mut self, damage: i32, _death_kind: DeathKind) {
        if self.dead {
            return;
        }
        if damage >= 1 {
            self.dead = true;
        }
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn is_dead(&self) -> bool {
        self.dead
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }

    /// Records the player x position so the next jump launches in the
    /// chase direction.
    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        ActiveInput, BackgroundEntity, BackgroundGrid, DeathKind, MessageDispatcher,
        MessageHandler, MessagePayload, MessageType, ObjectEntity, RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    struct SolidIf {
        solid: bool,
    }
    impl BackgroundEntity for SolidIf {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            !self.solid
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    fn grid_with_floor(width: usize, height: usize, floor_row: usize) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(height);
        for y in 0..height {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(width);
            for _ in 0..width {
                row.push(Box::new(SolidIf {
                    solid: y == floor_row,
                }));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn set_solid(grid: &mut BackgroundGrid, x: usize, y: usize, solid: bool) {
        grid.cells[y][x] = Box::new(SolidIf { solid });
    }

    fn synthetic_frog(x: i32, y: i32) -> openjill_data::jn::JnObject {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 22;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// `FrogEntity` reverses direction after hitting a wall.
    #[test]
    fn frog_reverses_direction_at_wall() {
        let cache = AssetCache::synthetic();
        let mut frog = FrogEntity::new(&synthetic_frog(32, 32), &cache);
        let mut backgrounds = grid_with_floor(8, 6, 3);
        set_solid(&mut backgrounds, 3, 2, true);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        frog.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(frog.bounding_box().x, 32, "blocked: no movement");
        assert!(frog.x_speed < 0, "direction reversed after wall");
    }

    /// `on_kill` with damage >= 1 marks dead; `draw` returns `None`.
    #[test]
    fn frog_on_kill_marks_dead_draw_returns_none() {
        let cache = AssetCache::synthetic();
        let mut frog = FrogEntity::new(&synthetic_frog(32, 32), &cache);
        assert!(frog.draw().is_some());
        frog.on_kill(1, DeathKind::Enemy);
        assert!(frog.dead);
        assert!(frog.draw().is_none(), "dead frog must not draw");
    }

    /// Score dispatched exactly once after death; second `update` skips it.
    #[test]
    fn dead_frog_score_dispatched_only_once() {
        struct Recorder(Arc<Mutex<usize>>);
        impl MessageHandler for Recorder {
            fn handle(&mut self, _: MessageType, _: &MessagePayload) {
                *self.0.lock().unwrap() += 1;
            }
        }

        let cache = AssetCache::synthetic();
        let mut frog = FrogEntity::new(&synthetic_frog(32, 32), &cache);
        frog.on_kill(1, DeathKind::Enemy);

        let count = Arc::new(Mutex::new(0usize));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::InventoryPoint,
            Box::new(Recorder(Arc::clone(&count))),
        );

        let grid = grid_with_floor(8, 8, 7);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        frog.update(&input, &state, &grid, &mut dispatcher);
        frog.update(&input, &state, &grid, &mut dispatcher);

        assert_eq!(*count.lock().unwrap(), 1, "score dispatched exactly once");
    }
}
