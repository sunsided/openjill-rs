//! Bullet / projectile entity (JN object type 36).
//!
//! Rust translation of `BulletObjectFactory` / `BulletManager` from the Java
//! reference (`open-jill-object-background`).
//!
//! A bullet moves by its stored `xd`/`yd` deltas every tick and removes
//! itself as soon as it overlaps a solid background cell or leaves the map
//! boundary.  It is used both for player-fired weapons and for the ten
//! colored die-burst bullets spawned by `PlayerEntity` on death.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundGrid, DeathKind,
    MessageDispatcher, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Bullet / projectile entity.
pub struct BulletEntity {
    /// World X position in pixels (top-left of the bounding box).
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick (positive = right).
    xd: i32,
    /// Vertical velocity in pixels per tick (positive = down).
    yd: i32,
    /// Set to `true` when the bullet should be removed from the object list.
    removed: bool,
}

impl BulletEntity {
    /// Builds a bullet entity from a JN object record.
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
            removed: false,
        }
    }

    /// Constructs a bullet with explicit position and velocity.
    ///
    /// Used by `BulletObjectFactory` when the player or an enemy spawns a
    /// bullet at runtime via a `CreateObject` dispatch rather than from a
    /// JN object record.
    pub fn with_velocity(x: i32, y: i32, w: i32, h: i32, xd: i32, yd: i32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            xd,
            yd,
            removed: false,
        }
    }
}

impl ObjectEntity for BulletEntity {
    /// Advances the bullet by one tick: integrates position and checks for
    /// termination (solid cell or out-of-map-bounds).
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
        self.x += self.xd;
        self.y += self.yd;

        let map_w = (BACKGROUND_GRID_WIDTH * BLOCK_SIZE_I as usize) as i32;
        let map_h = (BACKGROUND_GRID_HEIGHT * BLOCK_SIZE_I as usize) as i32;
        if self.x < 0 || self.x + self.w > map_w || self.y < 0 || self.y + self.h > map_h {
            self.removed = true;
            return;
        }

        if overlaps_solid(backgrounds, self.x, self.y, self.w, self.h) {
            self.removed = true;
        }
    }

    /// Sprite rendering deferred pending SHA tileset verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Bullets do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Any hit from a weapon or hazard removes the bullet immediately.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {
        self.removed = true;
    }

    /// Returns the bullet's bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` when the bullet has hit a solid cell or left the map.
    fn should_remove(&self) -> bool {
        self.removed
    }
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any
/// background cell that is not passable.
///
/// Used by `BulletEntity` to detect wall hits; passthrough and climbable
/// cells are transparent to bullets.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        BackgroundEntity, BackgroundGrid, MessageDispatcher, ObjectEntity, RenderCommand,
        RuntimeState,
    };

    /// Background cell variant used by the tests.
    #[derive(Clone, Copy)]
    enum CellKind {
        /// Open air: passable.
        Air,
        /// Solid block: not passable.
        Solid,
    }

    /// Test-only background cell.
    struct TestCell {
        /// Behavior flag.
        kind: CellKind,
    }

    impl BackgroundEntity for TestCell {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            matches!(self.kind, CellKind::Air)
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds a grid of the supplied `kind`.
    fn synthetic_grid(w: usize, h: usize, kind: CellKind) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(h);
        for _ in 0..h {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(w);
            for _ in 0..w {
                row.push(Box::new(TestCell { kind }));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Replaces cell `(x, y)` with one of `kind`.
    fn set_cell(grid: &mut BackgroundGrid, x: usize, y: usize, kind: CellKind) {
        grid.cells[y][x] = Box::new(TestCell { kind });
    }

    /// Unit under test: `BulletEntity` moves `xd` pixels per tick in x.
    ///
    /// Preconditions: bullet at `(16, 16)` with `xd = 4`, `yd = 0`; fully
    /// passable grid so no removal occurs.
    ///
    /// Invariants asserted: after one tick, x equals `16 + 4`.
    #[test]
    fn bullet_moves_xd_per_tick() {
        let grid = synthetic_grid(64, 64, CellKind::Air);
        let mut bullet = BulletEntity::with_velocity(16, 16, 8, 8, 4, 0);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        bullet.update(&input, &state, &grid, &mut dispatcher);

        assert_eq!(bullet.x, 20, "x must advance by xd after one tick");
        assert!(
            !bullet.should_remove(),
            "no solid cell; must not be removed"
        );
    }

    /// Unit under test: `BulletEntity` marks itself for removal on a solid
    /// background cell.
    ///
    /// Preconditions: bullet at `(16, 16)` moving right; solid cell at grid
    /// cell `(1, 1)` which the bullet enters after moving.
    ///
    /// Invariants asserted: after one tick the bullet reports `should_remove`.
    #[test]
    fn bullet_removes_on_solid_cell() {
        let mut grid = synthetic_grid(64, 64, CellKind::Air);
        // Bullet at (16,16) with xd=8: after one tick x=24, which maps to
        // cell (24/16, 16/16) = (1, 1).  Place a solid cell there.
        set_cell(&mut grid, 1, 1, CellKind::Solid);
        let mut bullet = BulletEntity::with_velocity(16, 16, 8, 8, 8, 0);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        bullet.update(&input, &state, &grid, &mut dispatcher);

        assert!(
            bullet.should_remove(),
            "bullet must be removed after hitting a solid cell"
        );
    }
}
