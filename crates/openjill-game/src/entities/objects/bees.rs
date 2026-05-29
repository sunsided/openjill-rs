//! Bees enemy entity (JN object type 46).
//!
//! Mirrors `org.jill.game.entities.obj.BeesManager`: a short-lived swarm that
//! weaves toward the player on both axes and harms the player on contact but
//! **cannot be killed** by weapons (`setKillabgeObject(false)`, no `msgKill`).
//! It self-destructs once its state counter reaches `stateDie`.
//!
//! Movement is driven by the `moveX` / `moveY` speed tables from
//! `object_conf.json`, indexed by `c = (state + 7) mod 32` (a 32-tick cycle).
//! Each tick the bee applies its current per-axis speed (snapping at walls via
//! [`slide_x`]/[`slide_y`]) and then picks the next speed magnitude from the
//! table, signed toward the player.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 37`.  The render slice
//! (tiles 10-11) is the port's reverse-engineered bee animation; only the
//! movement/lifetime behaviour is corrected here.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{EnemyRng, slide_x, slide_y};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the bees frames (`tileSet = 37`).
const TILESET_INDEX: u8 = 37;
/// Base tile index of the rendered bee animation slice.
const TILE_BASE: u16 = 10;
/// Number of animation frames cycled by the bees sprite.
const NUMBER_TILE_SET: u16 = 2;
/// State value at which the bee expires (`stateDie = 160`).
const STATE_DIE: i32 = 160;
/// Offset added to the state before the cycle modulo (`START_OFFSET`).
const START_OFFSET: i32 = 7;
/// Length of the movement cycle (`SIZE_OF_MVT`).
const SIZE_OF_MVT: i32 = 32;

/// One entry of a bee move table: speed magnitude for states below `bound`.
///
/// `hi > lo` selects a random magnitude in `[lo, hi)` each time the entry is
/// used (Java range syntax `lo-hi`); `hi == lo` is a fixed magnitude.
struct MoveStep {
    bound: i32,
    lo: i32,
    hi: i32,
}

/// `moveX = "4:1#11:0#15:1#32:2-4"`.
const MOVE_X: [MoveStep; 4] = [
    MoveStep {
        bound: 4,
        lo: 1,
        hi: 1,
    },
    MoveStep {
        bound: 11,
        lo: 0,
        hi: 0,
    },
    MoveStep {
        bound: 15,
        lo: 1,
        hi: 1,
    },
    MoveStep {
        bound: 32,
        lo: 2,
        hi: 4,
    },
];

/// `moveY = "4:0-3#11:0-4#15:0-3#27:0#31:0-1#32:0-2"`.
const MOVE_Y: [MoveStep; 6] = [
    MoveStep {
        bound: 4,
        lo: 0,
        hi: 3,
    },
    MoveStep {
        bound: 11,
        lo: 0,
        hi: 4,
    },
    MoveStep {
        bound: 15,
        lo: 0,
        hi: 3,
    },
    MoveStep {
        bound: 27,
        lo: 0,
        hi: 0,
    },
    MoveStep {
        bound: 31,
        lo: 0,
        hi: 1,
    },
    MoveStep {
        bound: 32,
        lo: 0,
        hi: 2,
    },
];

/// Returns the move magnitude for cycle value `c` from `table` (Java
/// `moveXorY`): the first entry whose `bound` exceeds `c` wins.
fn move_magnitude(c: i32, table: &[MoveStep], rng: &mut EnemyRng) -> i32 {
    for step in table {
        if c < step.bound {
            return if step.hi > step.lo {
                rng.range(step.lo, step.hi)
            } else {
                step.lo
            };
        }
    }
    0
}

/// Mixes spawn coordinates into a per-entity RNG seed.
fn seed_from(x: i32, y: i32) -> u32 {
    (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD737_4257)
}

pub struct BeesEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    player_x: i32,
    player_y: i32,
    x_speed: i32,
    y_speed: i32,
    /// Lifetime state counter; the bee is removed when it reaches [`STATE_DIE`].
    state: i32,
    counter: i32,
    removed: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    rng: EnemyRng,
    /// The JN object record this entity was built from (or a synthesized one
    /// for a hive-spawned bee), re-emitted by [`ObjectEntity::snapshot`] with
    /// the live position written back.
    origin: JnObject,
}

impl BeesEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "BeesEntity NUMBER_TILE_SET",
        );
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let x = i32::from(item.x());
        let y = i32::from(item.y());
        Self::build(x, y, w, h, item.clone())
    }

    /// Builds a `BeesEntity` spawned at runtime from a hive.
    ///
    /// Used by `LevelScreen::spawn_objects` when a `CreateObject` with
    /// `object_type = 46` arrives; bypasses the `JnObject` record because
    /// dynamically-spawned bees have no static JN record, synthesizing one so
    /// the bee still participates in save games.
    pub fn spawn_at(x: i32, y: i32) -> Self {
        let origin = JnObject::spawned(
            46,
            x as u16,
            y as u16,
            BLOCK_SIZE_I as u16,
            BLOCK_SIZE_I as u16,
        );
        Self::build(x, y, BLOCK_SIZE_I, BLOCK_SIZE_I, origin)
    }

    fn build(x: i32, y: i32, w: i32, h: i32, origin: JnObject) -> Self {
        Self {
            x,
            y,
            w,
            h,
            player_x: x,
            player_y: y,
            x_speed: 0,
            y_speed: 0,
            state: 0,
            counter: 0,
            removed: false,
            zaphold: 0,
            pending_kill: None,
            rng: EnemyRng::new(seed_from(x, y)),
            origin,
        }
    }
}

impl ObjectEntity for BeesEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        if self.removed {
            return;
        }
        if self.zaphold > 0 {
            self.zaphold -= 1;
        }

        // Lifetime: expire after `stateDie` ticks (Java removes itself).
        self.state += 1;
        if self.state >= STATE_DIE {
            self.removed = true;
            return;
        }

        self.counter = (self.counter + 1) % i32::from(NUMBER_TILE_SET);

        let c = (self.state + START_OFFSET).rem_euclid(SIZE_OF_MVT);
        let xd_sign = self.player_x - self.x;
        let yd_sign = self.player_y - self.y;

        // X: apply the speed chosen last tick, then pick the next magnitude
        // and sign it toward the player (Java `moveBeesOnX`).
        slide_x(
            backgrounds,
            &mut self.x,
            self.y,
            self.w,
            self.h,
            self.x_speed,
        );
        let mut new_xd = if xd_sign != 0 {
            move_magnitude(c, &MOVE_X, &mut self.rng)
        } else {
            0
        };
        if xd_sign < 0 {
            new_xd = -new_xd;
        }
        self.x_speed = new_xd;

        // Y: same, with the "same row -> flip current sign" rule (Java
        // `moveBeesOnY`).
        slide_y(
            backgrounds,
            self.x,
            &mut self.y,
            self.w,
            self.h,
            self.y_speed,
        );
        let mut new_yd = move_magnitude(c, &MOVE_Y, &mut self.rng);
        if yd_sign == 0 {
            if self.y_speed > 0 {
                new_yd = -new_yd;
            }
        } else if yd_sign < 0 {
            new_yd = -new_yd;
        }
        self.y_speed = new_yd;
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        let frame = (self.counter as u16).min(NUMBER_TILE_SET - 1);
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: TILE_BASE + frame,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.removed || self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
        self.pending_kill = Some(DeathKind::Enemy);
    }

    /// No-op: bees cannot be killed by weapons (`setKillabgeObject(false)`).
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Snapshots the live bee for a save game, or `None` once expired/removed.
    ///
    /// The bee re-acquires the player and rebuilds its flight on the next tick,
    /// so only the live position is written back over the cloned origin.
    fn snapshot(&self) -> Option<JnObject> {
        if self.removed {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }

    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
        self.player_y = player_bbox.y;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::BackgroundEntity;

    /// Open, passable air cell for movement tests.
    struct Air;
    impl BackgroundEntity for Air {
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

    fn open_grid(w: usize, h: usize) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(h);
        for _ in 0..h {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(w);
            for _ in 0..w {
                row.push(Box::new(Air));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn tick(bee: &mut BeesEntity, grid: &BackgroundGrid) {
        bee.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            grid,
            &mut MessageDispatcher::new(),
        );
    }

    /// The bee expires (requests removal) once its state reaches `stateDie`.
    #[test]
    fn bee_self_destructs_after_state_die() {
        let grid = BackgroundGrid::new(Vec::new());
        let mut bee = BeesEntity::spawn_at(100, 100);
        bee.observe_player(Rect::new(100, 100, 16, 16));
        for _ in 0..(STATE_DIE - 1) {
            tick(&mut bee, &grid);
            assert!(!bee.should_remove(), "bee alive before stateDie");
        }
        tick(&mut bee, &grid);
        assert!(bee.should_remove(), "bee removed at stateDie");
    }

    /// Weapons never kill a bee (`on_kill` is a no-op).
    #[test]
    fn bee_is_not_killable() {
        let mut bee = BeesEntity::spawn_at(0, 0);
        bee.on_kill(10, DeathKind::Enemy);
        assert!(!bee.should_remove(), "bee survives a weapon hit");
    }

    /// The bee weaves toward the player on both axes (open space, no walls).
    #[test]
    fn bee_homes_toward_player_both_axes() {
        let grid = open_grid(64, 64);
        let mut bee = BeesEntity::spawn_at(100, 100);
        // Player far to the lower-right.
        bee.observe_player(Rect::new(600, 600, 16, 16));
        let (x0, y0) = (bee.x, bee.y);
        for _ in 0..40 {
            tick(&mut bee, &grid);
        }
        assert!(bee.x > x0, "bee drifts right toward the player");
        assert!(bee.y > y0, "bee drifts down toward the player");
    }
}
