//! Crab enemy entity (JN object type 47).
//!
//! Mirrors `org.jill.game.entities.obj.CrabManager`: horizontal floor patrol
//! that reverses at walls and gaps and kills the player on contact, plus a
//! vine-climb state - while patrolling on a vine column the crab has a small
//! per-tick chance to start climbing, ascends until it hits a ceiling, then
//! descends until it lands and resumes patrol.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 38`, `tile = 0`,
//! `numberTileSet = 4`, `stateUpDown = 1`, `downUpMvtSize = 2`.
//! SHA header[38] confirms: crab sprites, 4 walk-cycle tiles.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{
    EnemyRng, blocked_ahead, floor_under_next, is_on_vine, slide_y, sprite_dims,
};
use crate::asset_cache::AssetCache;

const TILESET_INDEX: u8 = 38;
const TILE_BASE: u16 = 0;
const NUMBER_TILE_SET: u16 = 4;
const X_SPEED: i32 = 4;
const SCORE_VALUE: i32 = 200;
/// Vertical climb speed (`downUpMvtSize = 2`).
const CLIMB_SPEED: i32 = 2;
/// 1-in-N per-tick chance to start climbing while on a vine.
///
/// Mirrors Java `setState((int)(Math.random() * stateUpDown + 0.1))` with
/// `stateUpDown = 1`: the climb state (`state == 1`) is entered only when
/// `Math.random() >= 0.9`, i.e. roughly a 1-in-10 chance per tick.
const CLIMB_CHANCE_DEN: i32 = 10;

pub struct CrabEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    /// Vertical speed while climbing (negative up, positive down); `0` when
    /// patrolling.
    y_speed: i32,
    /// `true` while in the up/down vine-climb state.
    climbing: bool,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    rng: EnemyRng,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live state written back.
    origin: JnObject,
}

impl CrabEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let jn_h = i32::from(item.height());
        let y_adj = if jn_h > 0 { (h - jn_h).max(0) } else { 0 };
        let x = i32::from(item.x());
        let y = i32::from(item.y()) - y_adj;
        // Seed walk direction and animation phase from the JN record so a save
        // restores them; authored crabs carry zero speed (defaults to a
        // rightward patrol) and zero counter, leaving fresh entry unchanged.
        let xd = i32::from(item.x_speed());
        Self {
            x,
            y,
            w,
            h,
            x_speed: if xd != 0 { xd } else { X_SPEED },
            y_speed: i32::from(item.y_speed()),
            // A saved climbing crab carries a non-zero vertical speed; resume
            // the climb so save/load does not drop the up/down state.
            climbing: i32::from(item.y_speed()) != 0,
            counter: i32::from(item.counter()),
            dead: false,
            score_dispatched: false,
            zaphold: i32::from(item.zap_hold()),
            pending_kill: None,
            rng: EnemyRng::new((x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32)),
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for CrabEntity {
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

        if self.climbing {
            // Up/down state (Java `moveUpDown`): climb until a ceiling, then
            // reverse; on hitting a floor, resume patrol.
            let moved = slide_y(
                backgrounds,
                self.x,
                &mut self.y,
                self.w,
                self.h,
                self.y_speed,
            );
            if moved == 0 {
                if self.y_speed < 0 {
                    self.y_speed = -self.y_speed; // ceiling: turn around, descend
                } else {
                    self.climbing = false; // floor: back to patrol
                    self.y_speed = 0;
                }
            }
        } else {
            // Floor patrol: reverse at walls and gaps.
            if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
                && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            {
                self.x += self.x_speed;
            } else {
                self.x_speed = -self.x_speed;
            }

            // While on a vine column, small per-tick chance to start climbing
            // upward (Java's randomised state switch).
            if is_on_vine(backgrounds, self.x, self.y, self.w, self.h)
                && self.rng.range(0, CLIMB_CHANCE_DEN) == 0
            {
                self.climbing = true;
                self.y_speed = -CLIMB_SPEED;
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

    /// Snapshots the live crab for a save game, or `None` once dead.
    ///
    /// Writes back position (reversing the sprite y-adjustment), walk/climb
    /// speeds, the animation `counter`, and `zap_hold`; the transient climb
    /// sub-state re-derives on the next tick.
    fn snapshot(&self) -> Option<JnObject> {
        if self.dead {
            return None;
        }
        let mut obj = self.origin.clone();
        let jn_h = i32::from(obj.height());
        let y_adj = if jn_h > 0 { (self.h - jn_h).max(0) } else { 0 };
        obj.set_position(self.x as u16, (self.y + y_adj) as u16);
        // `new()` collapses an authored x_speed of 0 to the patrol default, so
        // a live speed equal to that default is ambiguous; emit the authored
        // value to keep the round-trip exact, and the live speed otherwise.
        let xs = if self.x_speed == X_SPEED {
            obj.x_speed()
        } else {
            self.x_speed as i16
        };
        obj.set_speed(xs, self.y_speed as i16);
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

    #[derive(Clone, Copy)]
    enum Cell {
        Air,
        Solid,
        Vine,
    }

    struct TestCell(Cell);
    impl BackgroundEntity for TestCell {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            !matches!(self.0, Cell::Solid)
        }
        fn is_climbable(&self) -> bool {
            matches!(self.0, Cell::Vine)
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    fn grid(w: usize, h: usize, f: impl Fn(usize, usize) -> Cell) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(h);
        for y in 0..h {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(w);
            for x in 0..w {
                row.push(Box::new(TestCell(f(x, y))));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn crab_at(x: i32, y: i32) -> CrabEntity {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 47;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = openjill_data::jn::JnFile::from_bytes(bytes).expect("synthetic JN parses");
        let cache = AssetCache::synthetic();
        let mut crab = CrabEntity::new(&jn.objects()[0], &cache);
        // Force the bounding box to one block so the column math is simple.
        crab.w = 16;
        crab.h = 16;
        crab.y = y;
        crab
    }

    fn tick(crab: &mut CrabEntity, g: &BackgroundGrid) {
        crab.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            g,
            &mut MessageDispatcher::new(),
        );
    }

    /// A crab standing on a vine column eventually starts climbing upward.
    #[test]
    fn crab_climbs_vine_then_reverses_at_ceiling() {
        // Column x=1 is a vine; row 0 is a solid ceiling; row 5 is the floor.
        let g = grid(4, 6, |x, y| {
            if y == 0 || y == 5 {
                Cell::Solid // ceiling (row 0) and floor (row 5)
            } else if x == 1 {
                Cell::Vine
            } else {
                Cell::Air
            }
        });
        // Block-aligned on the vine column, feet on the floor (row 5 top = 80).
        let mut crab = crab_at(16, 64);
        let start_y = crab.y;

        // Within a reasonable window the RNG should trigger a climb.
        let mut climbed = false;
        for _ in 0..200 {
            tick(&mut crab, &g);
            if crab.climbing && crab.y < start_y {
                climbed = true;
                break;
            }
        }
        assert!(climbed, "crab eventually climbs the vine upward");

        // Keep ticking; it must not tunnel through the ceiling (row 0).
        for _ in 0..200 {
            tick(&mut crab, &g);
            assert!(crab.y >= 16, "crab never rises past the ceiling cell");
        }
    }
}
