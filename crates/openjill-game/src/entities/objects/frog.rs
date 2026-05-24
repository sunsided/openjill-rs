//! Frog enemy entity (JN object type 22).
//!
//! Mirrors `org.jill.game.entities.obj.FrogManager`: horizontal patrol that
//! reverses direction at walls or gaps, jumps periodically, and kills the
//! player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 63`, `tile = 0`,
//! `numberTileSet = 6` (tiles 0-2 = floor walk, tiles 3-5 = jump arc).
//! SHA dump confirms: tileset 63 tile 0 is 14×10 px (6 tiles total).

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_below, floor_under_next, sprite_dims};
use crate::asset_cache::AssetCache;

const TILESET_INDEX: u8 = 63;
const NUMBER_TILE_SET: u16 = 6;
/// Frames per animation state (floor and jump each use 3 tiles).
const FRAMES_PER_STATE: u16 = 3;
const X_SPEED: i32 = 4;
const SCORE_VALUE: i32 = 100;
/// Ticks on the ground before initiating a jump (`counterBeforeJump`).
const JUMP_PERIOD: i32 = 17;
const JUMP_INIT_YD: i32 = -12;
const GRAVITY: i32 = 2;
/// Max downward speed per tick (`ySpeedMax`).
const FALL_SPEED_MAX: i32 = 12;
/// y_speed threshold below which the jump-arc tile set is shown
/// (`ySpeedChangePicture`).
const JUMP_Y_THRESHOLD: i32 = -10;

pub struct FrogEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    y_speed: i32,
    counter: i32,
    jump_counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
}

impl FrogEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
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
            counter: 0,
            jump_counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
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

        // Vertical (airborne arc).
        if self.y_speed != 0 || !floor_below(backgrounds, self.x, self.y, self.w, self.h) {
            self.y += self.y_speed;
            self.y_speed += GRAVITY;
            if self.y_speed > FALL_SPEED_MAX {
                self.y_speed = FALL_SPEED_MAX;
            }
            if self.y_speed > 0 && floor_below(backgrounds, self.x, self.y, self.w, self.h) {
                self.y_speed = 0;
            }
        }

        // Horizontal patrol.
        if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
        {
            self.x += self.x_speed;
        } else {
            self.x_speed = -self.x_speed;
        }

        // Periodic jump when grounded.
        if self.y_speed == 0 {
            self.jump_counter += 1;
            if self.jump_counter >= JUMP_PERIOD {
                self.jump_counter = 0;
                self.y_speed = JUMP_INIT_YD;
            }
        }

        self.counter += 1;
        if self.counter >= NUMBER_TILE_SET as i32 {
            self.counter = 0;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.dead {
            return None;
        }
        let frame = (self.counter as u16) % FRAMES_PER_STATE;
        let base = if self.y_speed < JUMP_Y_THRESHOLD {
            FRAMES_PER_STATE
        } else {
            0
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

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
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
