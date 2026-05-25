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
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Object type routed to [`ScatterParticleEntity::with_velocity`] in
/// [`crate::screens::level_screen::LevelScreen::spawn_objects`]. Chosen
/// outside the JN object-type range so runtime-only scatter particles
/// do not collide with any static record handled by `make_object_entity`.
pub const SCATTER_PARTICLE_TYPE: u8 = 100;

/// Dispatches an 8-direction scatter burst centred at `(cx, cy)`.
///
/// Each particle is spawned via a [`MessageType::CreateObject`] message
/// carrying [`SCATTER_PARTICLE_TYPE`] and one `(xd, yd)` from the
/// 8-direction spread. `LevelScreen::spawn_objects` constructs the
/// entity via [`ScatterParticleEntity::with_velocity`], which derives
/// the per-particle palette `state` offset from `(xd + yd) & 1` so the
/// burst shows two colour variants per rotation phase without changing
/// the `SpawnAt` payload shape.
pub fn spawn_burst_at(cx: i32, cy: i32, dispatcher: &mut MessageDispatcher) {
    /// Velocity tuples for the 8-direction spread.
    ///
    /// REVERSE-ENGINEERED from the Java `BulletObjectFactory`
    /// `xdRange = 7`, `xdRangeSubstract = 3`, `ydRange = 11`,
    /// `ydRangeSubstract = 8` (= velocities roughly in `[-3, 3]` x
    /// `[-8, 2]`). The Rust port uses a fixed 8-direction spread that
    /// covers the same envelope without per-frame randomness.
    const SPREAD: [(i32, i32); 8] = [
        (-3, -4),
        (-2, -6),
        (0, -7),
        (2, -6),
        (3, -4),
        (-3, -1),
        (3, -1),
        (0, -2),
    ];
    for (xd, yd) in SPREAD {
        dispatcher.send(
            MessageType::CreateObject,
            MessagePayload::SpawnAt {
                object_type: SCATTER_PARTICLE_TYPE,
                x: cx,
                y: cy,
                xd,
                yd,
            },
        );
    }
}

/// Number of palette variants used in [`tile_for_counter`] + state.
///
/// REVERSE-ENGINEERED: matches `bullet_factory.properties`
/// `stateRange = 2`.
const STATE_RANGE: u16 = 2;

/// Per-tick downward acceleration applied to the particle.
///
/// REVERSE-ENGINEERED: matches the Java `BulletManager.ySpeedMax = 12`
/// envelope and the `yd++` increment in `BulletManager.msgUpdate`.
const GRAVITY_PER_TICK: i32 = 1;

/// Pixel dimensions of the colored-bullet sprite in tileset 46.
///
/// REVERSE-ENGINEERED: tileset 46 carries 15 6×6 px frames. Sizing the
/// bullet-vs-collision bbox to match the sprite (instead of the default
/// 16×16 block) lets particles squeeze through the player's own
/// position and small terrain gaps without immediately self-removing on
/// the first tick.
const PARTICLE_SIZE: i32 = 6;

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
    /// Color offset added to the rotation tile, in `0..STATE_RANGE`.
    ///
    /// REVERSE-ENGINEERED: Java `BulletObjectFactory` seeds each spawned
    /// bullet with `setState((int)(Math.random() * stateRange))` and
    /// `BulletManager.msgDraw` returns
    /// `images[baseTile + getState()]`. The Rust port stores the per-
    /// particle offset here so a burst can show two distinct colour
    /// variants per rotation phase.
    state: u16,
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
            w: PARTICLE_SIZE,
            h: PARTICLE_SIZE,
            xd: i32::from(item.x_speed()),
            yd: i32::from(item.y_speed()),
            counter: 0,
            state: 0,
            removed: false,
        }
    }

    /// Constructs a scatter particle with explicit position and
    /// velocity. The palette `state` offset is derived deterministically
    /// from `(xd + yd) & 1` so two particles spawned with different
    /// directions land on different colour variants without needing a
    /// random number source.
    pub fn with_velocity(x: i32, y: i32, xd: i32, yd: i32) -> Self {
        let state = ((xd + yd).rem_euclid(STATE_RANGE as i32)) as u16;
        Self {
            x,
            y,
            w: PARTICLE_SIZE,
            h: PARTICLE_SIZE,
            xd,
            yd,
            counter: 0,
            state,
            removed: false,
        }
    }
}

impl ObjectEntity for ScatterParticleEntity {
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
        let nx = self.x + self.xd;
        let ny = self.y + self.yd;
        let map_w = (BACKGROUND_GRID_WIDTH * BLOCK_SIZE_I as usize) as i32;
        let map_h = (BACKGROUND_GRID_HEIGHT * BLOCK_SIZE_I as usize) as i32;
        if nx < 0 || nx + self.w > map_w || ny < 0 || ny + self.h > map_h {
            self.removed = true;
            return;
        }
        // Scatter particles are purely cosmetic; let them clip through
        // solid background cells so a burst spawned inside an enemy's
        // collision footprint is not killed on its first tick.
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
        // REVERSE-ENGINEERED: Java `BulletManager.msgDraw` returns
        // `images[baseTile + getState()]`. `tile_for_counter` provides
        // the rotation-based base tile; `state` adds the per-particle
        // colour offset seeded at spawn time.
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile: tile_for_counter(self.counter) + self.state,
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
