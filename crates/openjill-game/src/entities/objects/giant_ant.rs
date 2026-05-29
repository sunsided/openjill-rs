//! Giant ant enemy entity (JN object type 29).
//!
//! Mirrors `org.jill.game.entities.obj.GiantAntManager`: horizontal floor
//! patrol; reverses at walls and gaps; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 10`, `tile = 0`,
//! `numberTileSet = 4` (tiles 0-9 total; 0-3 used for walk cycle).
//! SHA header[10] confirms: 10 tiles, 32×16 px each.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_under_next, slide_x, sprite_dims};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the giant-ant frames.
///
/// REVERSE-ENGINEERED: `GiantAntManager.tileSet = 10` in `object_conf.json`.
/// Tileset 10 carries 10 tiles total; the Rust port animates the first 4.
/// The Java reference's `numberTileSet = 10` corresponds to the full
/// tileset, but the visible giant-ant walk cycle is 4 frames. Future
/// engine config file should expose this.
const TILESET_INDEX: u8 = 10;
/// Base tile index within [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED: `GiantAntManager.tile = 0` in `object_conf.json`.
const TILE_BASE: u16 = 0;
/// Number of animation frames cycled by the giant-ant sprite.
///
/// REVERSE-ENGINEERED: verified at construction by
/// [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: u16 = 4;
/// Horizontal patrol speed in pixels per tick.
///
/// REVERSE-ENGINEERED.
const X_SPEED: i32 = 4;
/// Score awarded when the giant ant is killed.
///
/// REVERSE-ENGINEERED.
const SCORE_VALUE: i32 = 200;
/// Ticks the ant pauses after turning at a wall/gap (`stateTurn = 2`).
const TURN_PAUSE: i32 = 2;

pub struct GiantAntEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    /// Remaining pause ticks after a turn; the ant holds still while `> 0`
    /// (Java `state = stateTurn`).
    turn_pause: i32,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live state written back.
    origin: JnObject,
}

impl GiantAntEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "GiantAntEntity NUMBER_TILE_SET",
        );
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let jn_h = i32::from(item.height());
        let y_adj = if jn_h > 0 { (h - jn_h).max(0) } else { 0 };
        // Seed walk direction and animation phase from the JN record; authored
        // ants carry zero speed (defaults to a rightward march) and zero
        // counter, so fresh level entry is unchanged.
        let xd = i32::from(item.x_speed());
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()) - y_adj,
            w,
            h,
            x_speed: if xd != 0 { xd } else { X_SPEED },
            turn_pause: 0,
            counter: i32::from(item.counter()),
            dead: false,
            score_dispatched: false,
            zaphold: i32::from(item.zap_hold()),
            pending_kill: None,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for GiantAntEntity {
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

        // Pause briefly after turning (Java state-machine `stateTurn`).
        if self.turn_pause > 0 {
            self.turn_pause -= 1;
            return;
        }

        if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
        {
            // Slide flush to the wall (Java `moveObjectRightOnFloor`).
            slide_x(
                backgrounds,
                &mut self.x,
                self.y,
                self.w,
                self.h,
                self.x_speed,
            );
        } else {
            // Wall or gap: reverse and pause.
            self.x_speed = -self.x_speed;
            self.turn_pause = TURN_PAUSE;
            self.counter = 0;
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
        if self.dead || self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
        self.pending_kill = Some(DeathKind::Enemy);
    }

    fn on_kill(&mut self, damage: i32, _death_kind: DeathKind) {
        if self.dead || damage < 1 {
            return;
        }
        self.dead = true;
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn is_dead(&self) -> bool {
        self.dead
    }

    /// Snapshots the live ant for a save game, or `None` once dead.
    ///
    /// Writes back position (reversing the sprite y-adjustment), march
    /// direction, the animation `counter`, and `zap_hold`; the transient turn
    /// pause re-derives on the next tick.
    fn snapshot(&self) -> Option<JnObject> {
        if self.dead {
            return None;
        }
        let mut obj = self.origin.clone();
        let jn_h = i32::from(obj.height());
        let y_adj = if jn_h > 0 { (self.h - jn_h).max(0) } else { 0 };
        obj.set_position(self.x as u16, (self.y + y_adj) as u16);
        // `new()` collapses an authored x_speed of 0 to the march default, so a
        // live speed equal to that default is ambiguous; emit the authored
        // value to keep the round-trip exact. The ant has no vertical motion,
        // so the authored y_speed is preserved.
        let xs = if self.x_speed == X_SPEED && obj.x_speed() == 0 {
            0
        } else {
            self.x_speed as i16
        };
        obj.set_speed(xs, obj.y_speed());
        obj.set_counter(self.counter as i16);
        obj.set_zap_hold(self.zaphold as u16);
        Some(obj)
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::BackgroundEntity;

    /// Background cell that is solid (a floor/wall) iff `0` is `true`.
    struct Solid(bool);
    impl BackgroundEntity for Solid {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            !self.0
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds a grid whose `floor_row` is solid and all other cells are air.
    fn grid_with_floor(width: usize, height: usize, floor_row: usize) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(height);
        for y in 0..height {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(width);
            for _ in 0..width {
                row.push(Box::new(Solid(y == floor_row)) as Box<dyn BackgroundEntity>);
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Regression for the snapshot speed-collapse guard: an ant authored
    /// marching left that reverses into the `+X_SPEED` default must snapshot
    /// the live (reversed, rightward) direction, not the authored leftward one.
    #[test]
    fn snapshot_persists_reversed_direction_not_authored() {
        let cache = AssetCache::synthetic();
        // Authored facing left, parked at x = 0 so the first step left is
        // blocked (off the map edge) and the ant reverses to +X_SPEED.
        let mut item = JnObject::spawned(29, 0, 96, 16, 16);
        item.set_speed(-X_SPEED as i16, 0);
        let mut ant = GiantAntEntity::new(&item, &cache);

        ant.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            &grid_with_floor(8, 8, 7),
            &mut MessageDispatcher::new(),
        );

        let snapshot = ant.snapshot().expect("a live ant snapshots");
        assert_eq!(
            snapshot.x_speed(),
            X_SPEED as i16,
            "snapshot must keep the live reversed direction, not the authored one"
        );
    }
}
