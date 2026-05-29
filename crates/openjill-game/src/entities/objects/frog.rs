//! Frog enemy entity (JN object type 22).
//!
//! Mirrors `org.jill.game.entities.obj.FrogManager`: two-state machine.
//! On floor (`on_floor = true`): counts ticks; after `counterBeforeJump`
//! ticks launches a gravity arc toward the player.  In air (`on_floor =
//! false`): slides horizontally (flush against walls, no direction reversal)
//! and vertically (flush against ceilings/floors via [`slide_y`]), applies
//! gravity (+1 per tick), and lands when a downward slide is fully blocked.
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

use super::enemy_shared::{slide_x, slide_y, sprite_dims};
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
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live state written back.
    origin: JnObject,
}

/// JN `state` value for a frog resting on the floor
/// (`FrogManager.stateOnFloor = 0`).
const STATE_ON_FLOOR: i16 = 0;
/// JN `state` value for an airborne frog (`FrogManager.stateOnJump = 1`).
const STATE_ON_JUMP: i16 = 1;

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
        // Seed the live state from the JN record so a save game restores
        // mid-jump: `state` selects floor vs air, `counter` is the jump timer,
        // and the speeds carry the in-flight arc.  Authored frogs carry
        // `state = 0` (floor), `counter = 0`, and zero speeds, so a freshly
        // loaded level is unchanged.
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()) - y_adj,
            w,
            h,
            x_speed: i32::from(item.x_speed()),
            y_speed: i32::from(item.y_speed()),
            jump_counter: i32::from(item.counter()),
            on_floor: item.state() != STATE_ON_JUMP,
            dead: false,
            score_dispatched: false,
            zaphold: i32::from(item.zap_hold()),
            pending_kill: None,
            player_x: i32::from(item.x()),
            origin: item.clone(),
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
            // Floor state: the frog sits still and only counts ticks. Java
            // `FrogManager.msgUpdate` does no horizontal movement while
            // grounded (the floor branch is just `this.counter++`); it waits
            // for `counterBeforeJump`, then launches toward the player.
            self.jump_counter += 1;
            if self.jump_counter >= JUMP_PERIOD {
                self.jump_counter = 0;
                self.y_speed = JUMP_INIT_YD;
                self.on_floor = false;
                // Java `FrogManager.msgUpdate`: at jump launch reset
                // `xSpeed = xSpeedMax` and flip the sign when the player
                // is to the left (`xd = PLAYER_POSITION.getX() - this.x;
                // if (xd < 0) this.xSpeed *= -1;`). This makes the frog
                // chase the player horizontally.
                let dir = if self.player_x < self.x { -1 } else { 1 };
                self.x_speed = X_SPEED * dir;
                // Java `this.y--` at launch ("picture has greater size").
                self.y -= 1;
            }
        } else {
            // Air state: mirrors Java `FrogManager.msgUpdate`'s airborne branch.
            // Horizontal: slide flush, stopping at walls (no reversal in air).
            slide_x(
                backgrounds,
                &mut self.x,
                self.y,
                self.w,
                self.h,
                self.x_speed,
            );

            // Vertical: slide flush per axis so the frog cannot pass through a
            // ceiling on the way up nor overshoot into the floor on the way
            // down (Java `moveObjectUp`/`moveObjectDown`).
            let moved = slide_y(
                backgrounds,
                self.x,
                &mut self.y,
                self.w,
                self.h,
                self.y_speed,
            );
            if self.y_speed < 0 && moved == 0 {
                // Hit a ceiling while rising: cancel upward speed
                // (`if (!moveObjectUp(...)) ySpeed = Y_SPEED_MIDDLE`).
                self.y_speed = 0;
            } else if self.y_speed > 0 && moved == 0 {
                // Could not descend: landed (`if (!moveObjectDown(...))
                // state = stateOnFloor; counter = 0`).
                self.y_speed = 0;
                self.on_floor = true;
                self.jump_counter = 0;
            }

            // Gravity, applied after the move (`if (ySpeed < ySpeedMax) ySpeed++`).
            if self.y_speed < FALL_SPEED_MAX {
                self.y_speed += 1;
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

    /// Snapshots the live frog for a save game, or `None` once dead.
    ///
    /// Writes back the `FrogManager` `ObjectItem` fields: `state` (floor/jump),
    /// `counter` (jump timer), the speeds, and `zap_hold`.  The construction
    /// y-adjustment (sprite height vs JN height) is reversed so the position
    /// round-trips.
    fn snapshot(&self) -> Option<JnObject> {
        if self.dead {
            return None;
        }
        let mut obj = self.origin.clone();
        let jn_h = i32::from(obj.height());
        let y_adj = if jn_h > 0 { (self.h - jn_h).max(0) } else { 0 };
        obj.set_position(self.x as u16, (self.y + y_adj) as u16);
        obj.set_speed(self.x_speed as i16, self.y_speed as i16);
        obj.set_state(if self.on_floor {
            STATE_ON_FLOOR
        } else {
            STATE_ON_JUMP
        });
        obj.set_counter(self.jump_counter as i16);
        obj.set_zap_hold(self.zaphold as u16);
        Some(obj)
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

    /// Grounded `FrogEntity` stays still and only counts ticks, then launches
    /// toward the player after `JUMP_PERIOD` ticks (Java `FrogManager`:
    /// floor state does no horizontal movement, just `counter++`).
    #[test]
    fn frog_waits_on_floor_then_jumps_toward_player() {
        let cache = AssetCache::synthetic();
        let mut frog = FrogEntity::new(&synthetic_frog(64, 32), &cache);
        let backgrounds = grid_with_floor(8, 6, 3);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        // Player to the left of the frog.
        frog.observe_player(Rect::new(0, 32, 16, 16));

        // While grounded the frog does not move horizontally.
        for _ in 0..(JUMP_PERIOD - 1) {
            frog.update(&input, &state, &backgrounds, &mut dispatcher);
            assert_eq!(frog.bounding_box().x, 64, "grounded frog must not patrol");
            assert!(frog.on_floor, "frog still grounded before jump");
        }

        // The launch tick fires the jump toward the player (to the left).
        frog.update(&input, &state, &backgrounds, &mut dispatcher);
        assert!(!frog.on_floor, "frog launched into the air");
        assert!(frog.x_speed < 0, "frog jumps toward the player on the left");
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

    /// Unit under test: [`FrogEntity::snapshot`] round-trips an airborne frog
    /// (`parse -> new -> snapshot == parse`).
    ///
    /// The record carries the air state, a jump-timer `counter`, a non-zero
    /// arc velocity, `zap_hold`, and a sub-sprite height that exercises the
    /// construction y-adjustment reversal.
    #[test]
    fn snapshot_round_trips_an_airborne_frog() {
        let cache = AssetCache::synthetic();
        let mut obj = openjill_data::jn::JnObject::spawned(22, 80, 96, 14, 2);
        obj.set_state(STATE_ON_JUMP);
        obj.set_counter(5);
        obj.set_speed(-4, 3);
        obj.set_zap_hold(2);

        let frog = FrogEntity::new(&obj, &cache);
        assert_eq!(frog.snapshot(), Some(obj.clone()));
    }
}
