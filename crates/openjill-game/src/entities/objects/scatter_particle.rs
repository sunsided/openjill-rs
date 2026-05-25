//! Scatter-particle entity used for enemy death-burst effects.
//!
//! Lightweight ballistic projectile spawned by entities such as the
//! firebird when they die: integrates `xd`/`yd` per tick with gravity,
//! self-removes on wall contact, map-bounds exit, or after a fixed
//! lifetime. Mirrors the Java reference `BulletObjectFactory.explode`
//! visual effect (small colored bullets scattering from the impact
//! position) without the boomerang state machine that lives in
//! [`crate::entities::objects::BulletEntity`].
//!
//! Particles do not damage the player or enemies — they exist purely as
//! a visual effect. Pickup semantics (turning the scattered particles
//! into collectable gems) are tracked separately in `PORT-FINDINGS.md`
//! under the firebird gem-scatter entry.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundGrid, DeathKind,
    MessageDispatcher, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Per-tick downward acceleration applied to the particle.
///
/// REVERSE-ENGINEERED: matches the Java `BulletManager.ySpeedMax = 12`
/// envelope and the `yd++` increment in `BulletManager.msgUpdate`.
const GRAVITY_PER_TICK: i32 = 1;

/// Maximum downward speed clamp.
///
/// REVERSE-ENGINEERED: `BulletManager.ySpeedMax = 12` in
/// `object_conf.json`.
const Y_SPEED_MAX: i32 = 12;

/// Maximum ticks the particle stays alive before self-removing.
///
/// REVERSE-ENGINEERED: `BulletManager.counterDie = 40` in
/// `object_conf.json`.
const COUNTER_DIE: i32 = 40;

/// SHA tileset that owns the scatter-particle frames.
///
/// REVERSE-ENGINEERED: `BulletManager.tileSet = 46` carries 15 6×6 px
/// colored particles.
const TILESET: u8 = 46;

/// Number of distinct particle sprites cycled by [`tile_for_counter`].
///
/// REVERSE-ENGINEERED: derived from the Java `tileByState =
/// "8:12#16:9#24:6#32:3#40:0"` schedule, which steps through five
/// distinct frames over the 40-tick lifetime.
const FRAME_COUNT: i32 = 5;

/// Scatter-particle entity.
pub struct ScatterParticleEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels (matches the particle sprite width).
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick (positive = right).
    xd: i32,
    /// Vertical velocity in pixels per tick (positive = down).
    yd: i32,
    /// Tick counter; drives the rotating-particle frame selection and
    /// the [`COUNTER_DIE`] lifetime cutoff.
    counter: i32,
    /// Set to `true` once the particle should be reaped.
    removed: bool,
}

impl ScatterParticleEntity {
    /// Constructs a scatter particle from a JN object record (unused in
    /// practice but kept for factory uniformity).
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w: BLOCK_SIZE_I,
            h: BLOCK_SIZE_I,
            xd: i32::from(item.x_speed()),
            yd: i32::from(item.y_speed()),
            counter: 0,
            removed: false,
        }
    }

    /// Constructs a scatter particle with explicit position and velocity.
    pub fn with_velocity(x: i32, y: i32, xd: i32, yd: i32) -> Self {
        Self {
            x,
            y,
            w: BLOCK_SIZE_I,
            h: BLOCK_SIZE_I,
            xd,
            yd,
            counter: 0,
            removed: false,
        }
    }
}

impl ObjectEntity for ScatterParticleEntity {
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
        let nx = self.x + self.xd;
        let ny = self.y + self.yd;
        let map_w = (BACKGROUND_GRID_WIDTH * BLOCK_SIZE_I as usize) as i32;
        let map_h = (BACKGROUND_GRID_HEIGHT * BLOCK_SIZE_I as usize) as i32;
        if nx < 0 || nx + self.w > map_w || ny < 0 || ny + self.h > map_h {
            self.removed = true;
            return;
        }
        if overlaps_solid(backgrounds, nx, ny, self.w, self.h) {
            self.removed = true;
            return;
        }
        self.x = nx;
        self.y = ny;
        if self.yd < Y_SPEED_MAX {
            self.yd += GRAVITY_PER_TICK;
        }
        self.counter += 1;
        if self.counter >= COUNTER_DIE {
            self.removed = true;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile: tile_for_counter(self.counter),
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {
        self.removed = true;
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Particles tick off-screen so the death-burst completes even when
    /// the camera scrolls away mid-flight.
    fn always_active(&self) -> bool {
        true
    }
}

/// Returns the rotating-particle tile index for the supplied tick
/// counter, mirroring the Java `tileByState` schedule
/// `"8:12#16:9#24:6#32:3#40:0"` collapsed onto [`FRAME_COUNT`] frames
/// in tileset 46.
fn tile_for_counter(counter: i32) -> u16 {
    let bucket = (counter.max(0) / 8).min(FRAME_COUNT - 1);
    match bucket {
        0 => 12,
        1 => 9,
        2 => 6,
        3 => 3,
        _ => 0,
    }
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any
/// background cell that is not passable.
fn overlaps_solid(grid: &BackgroundGrid, x: i32, y: i32, w: i32, h: i32) -> bool {
    let cx_l = x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cx_r = ((x + w - 1).div_euclid(BLOCK_SIZE_I)).max(0) as usize;
    let cy_t = y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cy_b = ((y + h - 1).div_euclid(BLOCK_SIZE_I)).max(0) as usize;
    for cy in cy_t..=cy_b {
        if cy >= grid.height {
            continue;
        }
        for cx in cx_l..=cx_r {
            if cx >= grid.width {
                continue;
            }
            if let Some(cell) = grid.get(cx, cy)
                && !cell.is_passthrough()
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        BackgroundEntity, BackgroundGrid, MessageDispatcher, ObjectEntity, RenderCommand,
        RuntimeState,
    };

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

    fn air_grid(w: usize, h: usize) -> BackgroundGrid {
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

    /// Unit under test: scatter particle integrates xd/yd with gravity.
    #[test]
    fn particle_falls_under_gravity() {
        let grid = air_grid(64, 64);
        let mut p = ScatterParticleEntity::with_velocity(100, 100, 3, -4);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        p.update(&input, &state, &grid, &mut dispatcher);
        assert_eq!(p.x, 103);
        assert_eq!(p.y, 96);
        assert_eq!(p.yd, -3, "yd must increment by GRAVITY_PER_TICK each step");
    }

    /// Unit under test: scatter particle self-removes after COUNTER_DIE ticks.
    #[test]
    fn particle_self_removes_after_counter_die() {
        let grid = air_grid(64, 64);
        let mut p = ScatterParticleEntity::with_velocity(100, 100, 0, 0);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        for _ in 0..COUNTER_DIE {
            p.update(&input, &state, &grid, &mut dispatcher);
        }
        assert!(p.should_remove());
    }
}
