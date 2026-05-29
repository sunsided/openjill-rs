//! Bullet / thrown-knife projectile entity (JN object type 36).
//!
//! Rust translation of the Java reference `KniveManager` boomerang state
//! machine (the Java port reuses the bullet object slot for player-spawned
//! knives via `BulletObjectFactory`). The projectile flies linearly for
//! [`LAUNCH_END`] ticks, homes toward the player for [`FOLLOW_MAX`] ticks
//! (boomerang return), then falls under gravity. Hitting an enemy in any
//! phase past [`NO_MOVE_NO_HIT`] immediately starts the return phase, so
//! the thrown knife snaps back to the player after a kill.
//!
//! Wall behaviour mirrors the Java `moveObject*` helpers: during the
//! initial launch the projectile self-removes on contact with a solid
//! cell, but during the follow phase walls only stop motion so the
//! return trajectory is not lost.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundGrid, DeathKind,
    InventoryItemPayload, InventoryObject, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Statecount value where the projectile neither moves nor hits anything.
///
/// REVERSE-ENGINEERED: `KniveManager.statecountNoMoveNoHit = 0` in
/// `object_conf.json`. Used by the on-hit return trigger to skip the
/// transition when the projectile is dormant.
const NO_MOVE_NO_HIT: i32 = 0;
/// First statecount value of the linear-launch phase.
///
/// REVERSE-ENGINEERED: `KniveManager.statecountLaunchStart = 1`.
const LAUNCH_START: i32 = 1;
/// Last statecount value of the linear-launch phase.
///
/// REVERSE-ENGINEERED: `KniveManager.statecountLaunchEnd = 14`.
const LAUNCH_END: i32 = 14;
/// Last statecount value of the follow-player (boomerang return) phase.
///
/// REVERSE-ENGINEERED: `KniveManager.statecountFollowPlayerMax = 64`.
const FOLLOW_MAX: i32 = 64;
/// Maximum rightward horizontal speed during the follow phase.
///
/// REVERSE-ENGINEERED: `KniveManager.leftMaxMoveX = 8` (clamp on the
/// positive side; the Java field is named for the launch direction and is
/// reused as the clamp magnitude during homing).
const RIGHT_MAX_X: i32 = 8;
/// Maximum leftward horizontal speed during the follow phase.
///
/// REVERSE-ENGINEERED: `KniveManager.rightMaxMoveX = -8`.
const LEFT_MAX_X: i32 = -8;
/// Maximum downward vertical speed during the follow phase.
///
/// REVERSE-ENGINEERED: `KniveManager.downMaxMoveY = 4`.
const DOWN_MAX_Y: i32 = 4;
/// Maximum upward vertical speed during the follow phase.
///
/// REVERSE-ENGINEERED: `KniveManager.upMaxMoveY = -4`.
const UP_MAX_Y: i32 = -4;
/// Per-tick downward step during the fall phase.
///
/// REVERSE-ENGINEERED: `KniveManager.moveDown = 1`.
const FALL_STEP_Y: i32 = 1;
/// SHA tileset that owns the knife frames.
///
/// REVERSE-ENGINEERED: `KNIVE = "14,13"` in `object_conf.json` (tile 14,
/// tileset 13).
const KNIFE_TILESET: u8 = 13;
/// Width/height of a thrown-knife sprite in pixels.
///
/// REVERSE-ENGINEERED: SHA tileset 13 carries five 10x10 px knife frames.
/// Java `KniveManager` sizes its bounding box from the image, so the thrown
/// knife collides as a 10x10 box rather than a full 16x16 block.
pub(crate) const KNIFE_W: i32 = 10;
pub(crate) const KNIFE_H: i32 = 10;

/// Thrown-knife / bullet projectile entity.
pub struct BulletEntity {
    /// World X position in pixels (top-left of the bounding box).
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick (positive = right). Drives
    /// the linear launch in pure form and is adjusted toward the player
    /// inside the follow phase.
    xd: i32,
    /// Vertical velocity in pixels per tick (positive = down). Adjusted
    /// toward the player during the follow phase, snapped to
    /// [`FALL_STEP_Y`] in the fall phase.
    yd: i32,
    /// Boomerang state counter, incremented every tick. Drives the
    /// launch → follow → fall phase transitions per the Java
    /// `KniveManager` state machine.
    state_count: i32,
    /// Most recent player bounding box observed via
    /// [`ObjectEntity::observe_player`], used by the follow phase to
    /// home toward the player and to detect follow-phase player catches
    /// without needing a direct entity reference.
    player_bbox: Rect,
    /// Set to `true` when the projectile should be removed from the
    /// object list.
    removed: bool,
}

impl BulletEntity {
    /// Builds a bullet entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let x = i32::from(item.x());
        let y = i32::from(item.y());
        Self {
            x,
            y,
            w,
            h,
            xd: i32::from(item.x_speed()),
            yd: i32::from(item.y_speed()),
            state_count: LAUNCH_START,
            player_bbox: Rect::new(x, y, w, h),
            removed: false,
        }
    }

    /// Constructs a bullet with explicit position and velocity, starting
    /// in the launch phase (`state_count = LAUNCH_START`).
    ///
    /// Used by `LevelScreen::spawn_objects` when the player presses the
    /// throw key, mirroring the Java reference's `BulletObjectFactory`
    /// runtime spawn path.
    pub fn with_velocity(x: i32, y: i32, w: i32, h: i32, xd: i32, yd: i32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            xd,
            yd,
            state_count: LAUNCH_START,
            player_bbox: Rect::new(x, y, w, h),
            removed: false,
        }
    }
}

impl ObjectEntity for BulletEntity {
    /// Advances the projectile by one tick.
    ///
    /// Dispatches to the launch, follow, or fall phase based on
    /// `state_count`, then increments the counter. Launch-phase wall
    /// contact removes the projectile immediately; follow-phase wall
    /// contact only stops further motion so the boomerang return is
    /// preserved. The fall phase removes the projectile once it leaves
    /// the map bounds or hits a solid cell.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.removed {
            return;
        }

        // Landed pickup: sit at rest and wait for the player to walk
        // over the projectile. Mirrors Java `KniveManager`'s
        // post-`moveDown` `stateCount = 0` (`NoMoveNoHit`) state where
        // `msgUpdate`'s state machine has no branch and the knife
        // becomes pickup-able via `msgTouch`.
        if self.state_count == NO_MOVE_NO_HIT {
            if self.bounding_box().intersects(&self.player_bbox) {
                dispatch_catch(dispatcher);
                self.removed = true;
            }
            return;
        }

        if (LAUNCH_START..=LAUNCH_END).contains(&self.state_count) {
            self.tick_launch(backgrounds);
        } else if self.state_count > LAUNCH_END && self.state_count <= FOLLOW_MAX {
            self.tick_follow(backgrounds);
        } else if self.state_count > FOLLOW_MAX {
            self.tick_fall(backgrounds);
        }

        // Follow/fall-phase overlap with the player counts as the
        // player catching the boomerang knife: return the inventory
        // item and remove the projectile from the active list.
        if !self.removed
            && self.state_count > LAUNCH_END
            && self.bounding_box().intersects(&self.player_bbox)
        {
            dispatch_catch(dispatcher);
            self.removed = true;
        }

        // Advance the state machine. `tick_fall` may have snapped the
        // counter back to `NO_MOVE_NO_HIT` on floor contact; suppress
        // the increment so the landed knife sits still next tick.
        if self.state_count != NO_MOVE_NO_HIT {
            self.state_count = self.state_count.saturating_add(1);
        }
    }

    /// Renders the projectile using the 5-frame rotating-knife sprite
    /// in tileset 13.
    ///
    /// REVERSE-ENGINEERED: tileset 13 carries five 10×10 px knife
    /// frames. The Java reference `KniveManager` loads only tile 0
    /// (`msgDraw` returns a single image), but the original DOS game
    /// animates the spin across all 5 frames; the Rust port cycles
    /// through them at one frame per two ticks for a visible rotation.
    /// Future engine config file should expose the spin period.
    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        let frame = ((self.state_count.max(0) / 2) as u16) % 5;
        Some(RenderCommand::Blit {
            tileset: KNIFE_TILESET,
            tile: frame,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Projectiles do not react to direct player overlap here; the level
    /// loop's projectile-vs-enemy hit pass handles enemy damage and
    /// follow-phase player overlap is treated as a successful catch
    /// inside [`Self::observe_player`].
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Returns `true` so the level loop's projectile-vs-enemy hit pass
    /// can identify this object as a player-spawned projectile that
    /// should damage enemies on overlap.
    fn is_projectile(&self) -> bool {
        true
    }

    /// Returns `true` so the boomerang state machine keeps advancing
    /// even when the projectile leaves the viewport update rectangle.
    /// Without this, a launched knife that flies past the viewport edge
    /// would have its `state_count` frozen and never transition into
    /// the follow phase, so it would never return.
    fn always_active(&self) -> bool {
        true
    }

    /// Captures the player bounding box for the follow-phase homing pass
    /// and the in-update catch detection. Catch handling lives in
    /// [`Self::update`] (not here) so the catch can dispatch the
    /// `InventoryItem(add Knife)` message; this hook has no dispatcher.
    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_bbox = player_bbox;
    }

    /// Snaps the projectile into the follow (boomerang return) phase.
    ///
    /// Called by the level loop's projectile-vs-enemy hit pass when this
    /// projectile lands a hit on an enemy: per Java `KniveManager.msgTouch`,
    /// `setStateCount(statecountLaunchEnd + 1)` makes the knife immediately
    /// stop the linear launch and start homing back to the player rather
    /// than continuing through walls.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {
        if self.state_count != NO_MOVE_NO_HIT && self.state_count <= LAUNCH_END {
            self.state_count = LAUNCH_END + 1;
        }
    }

    /// Returns the projectile's bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the projectile has been removed.
    fn should_remove(&self) -> bool {
        self.removed
    }
}

impl BulletEntity {
    /// Launch phase: linear motion in `xd`/`yd`. Wall contact and map
    /// edges both only stop further motion this tick — the projectile
    /// is NOT removed — so the follow phase can still take over once
    /// `state_count` passes [`LAUNCH_END`] and pull the knife back to
    /// the player. Mirrors Java `KniveManager.moveLeftRight`: the
    /// underlying `UtilityObjectEntity.moveObject{Left,Right}` blocks
    /// at walls without killing the entity, and the Java port's map
    /// is bounded such that the player cannot throw past it. Removing
    /// the projectile on map exit would permanently consume the
    /// player's only knife when throwing near the screen edge.
    fn tick_launch(&mut self, backgrounds: &BackgroundGrid) {
        self.step_x(backgrounds, self.xd);
        self.step_y(backgrounds, self.yd);
    }

    /// Follow phase: adjust `xd`/`yd` one pixel per tick toward the last
    /// observed player position, clamped to the per-axis maximum speeds, then
    /// move each axis independently so the boomerang slides flush along a wall
    /// back toward the player instead of wedging when one axis is blocked.
    /// Mirrors Java `KniveManager.followPlayer`, which calls `moveLeftRight`
    /// and `moveUpDown` as two separate snapping moves.
    fn tick_follow(&mut self, backgrounds: &BackgroundGrid) {
        let (px, py) = (self.player_bbox.x, self.player_bbox.y);
        if self.x < px && self.xd < RIGHT_MAX_X {
            self.xd += 1;
        } else if self.x > px && self.xd > LEFT_MAX_X {
            self.xd -= 1;
        }
        if self.y < py && self.yd < DOWN_MAX_Y {
            self.yd += 1;
        } else if self.y > py && self.yd > UP_MAX_Y {
            self.yd -= 1;
        }
        self.step_x(backgrounds, self.xd);
        self.step_y(backgrounds, self.yd);
    }

    /// Slides the projectile horizontally by `dx`, one pixel at a time, stopping
    /// flush against a solid cell or the map edge.  Mirrors Java
    /// `UtilityObjectEntity.moveObjectLeft`/`moveObjectRight` (a partial move
    /// that snaps to the wall).
    ///
    /// Only the *leading* edge column is tested for a wall, not the whole box,
    /// so a knife that spawned embedded in a wall (thrown while flush against
    /// it) can still slide out toward the open side instead of being wedged.
    fn step_x(&mut self, backgrounds: &BackgroundGrid, dx: i32) {
        let step = dx.signum();
        for _ in 0..dx.abs() {
            let nx = self.x + step;
            if !self.in_map(nx, self.y) {
                break;
            }
            let probe_x = if step > 0 { nx + self.w - 1 } else { nx };
            if column_solid(backgrounds, probe_x, self.y, self.h) {
                break;
            }
            self.x = nx;
        }
    }

    /// Slides the projectile vertically by `dy`, one pixel at a time, stopping
    /// flush against a solid cell or the map edge.  Mirrors Java
    /// `moveObjectUp`/`moveObjectDown`.  Tests only the leading edge row so an
    /// embedded projectile can slide out.
    fn step_y(&mut self, backgrounds: &BackgroundGrid, dy: i32) {
        let step = dy.signum();
        for _ in 0..dy.abs() {
            let ny = self.y + step;
            if !self.in_map(self.x, ny) {
                break;
            }
            let probe_y = if step > 0 { ny + self.h - 1 } else { ny };
            if row_solid(backgrounds, self.x, probe_y, self.w) {
                break;
            }
            self.y = ny;
        }
    }

    /// Fall phase: gravity. Snaps vertical speed to [`FALL_STEP_Y`].
    /// Floor contact snaps `state_count` back to [`NO_MOVE_NO_HIT`] so
    /// the knife sits at rest as a recoverable pickup (mirrors Java
    /// `KniveManager.moveDown`: when `moveObjectDown` returns `false`
    /// it sets `stateCount = 0`). Map-bottom exit removes the
    /// projectile permanently since an off-map knife cannot be picked
    /// up.
    fn tick_fall(&mut self, backgrounds: &BackgroundGrid) {
        self.yd = FALL_STEP_Y;
        let ny = self.y + self.yd;
        if !self.in_map(self.x, ny) {
            self.removed = true;
            return;
        }
        if overlaps_solid(backgrounds, self.x, ny, self.w, self.h) {
            self.state_count = NO_MOVE_NO_HIT;
            self.yd = 0;
            return;
        }
        self.y = ny;
    }

    /// Returns `true` when the projectile's bounding box at `(x, y)` is
    /// fully inside the map grid.
    fn in_map(&self, x: i32, y: i32) -> bool {
        let map_w = (BACKGROUND_GRID_WIDTH * BLOCK_SIZE_I as usize) as i32;
        let map_h = (BACKGROUND_GRID_HEIGHT * BLOCK_SIZE_I as usize) as i32;
        x >= 0 && x + self.w <= map_w && y >= 0 && y + self.h <= map_h
    }
}

/// Dispatches the `InventoryItem(add Knife)` message used when the
/// player catches a returning boomerang or walks over a landed knife.
fn dispatch_catch(dispatcher: &mut MessageDispatcher) {
    dispatcher.send(
        MessageType::InventoryItem,
        MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::Knife)),
    );
}

/// Returns `true` when the rectangle `[x, x+w) x [y, y+h)` overlaps any
/// background cell that is not passable.
///
/// Used by `BulletEntity` to detect wall hits; passthrough and climbable
/// cells are transparent to projectiles.
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

/// Returns `true` when the single cell column containing pixel `col_x` has a
/// solid (non-passthrough) cell anywhere across the vertical span `[y, y+h)`.
///
/// Used for the leading-edge wall test when sliding horizontally so a knife
/// embedded in a wall is not wedged - only the edge it is advancing into
/// blocks the move.
fn column_solid(grid: &BackgroundGrid, col_x: i32, y: i32, h: i32) -> bool {
    let cx = col_x.div_euclid(BLOCK_SIZE_I);
    if cx < 0 || cx as usize >= grid.width {
        return false;
    }
    let cx = cx as usize;
    let cy_t = y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cy_b = ((y + h - 1).div_euclid(BLOCK_SIZE_I)).max(0) as usize;
    for cy in cy_t..=cy_b {
        if cy >= grid.height {
            continue;
        }
        if let Some(cell) = grid.get(cx, cy)
            && !cell.is_passthrough()
        {
            return true;
        }
    }
    false
}

/// Returns `true` when the single cell row containing pixel `row_y` has a
/// solid cell anywhere across the horizontal span `[x, x+w)`.  Leading-edge
/// counterpart of [`column_solid`] for vertical slides.
fn row_solid(grid: &BackgroundGrid, x: i32, row_y: i32, w: i32) -> bool {
    let cy = row_y.div_euclid(BLOCK_SIZE_I);
    if cy < 0 || cy as usize >= grid.height {
        return false;
    }
    let cy = cy as usize;
    let cx_l = x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
    let cx_r = ((x + w - 1).div_euclid(BLOCK_SIZE_I)).max(0) as usize;
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

    /// Unit under test: `BulletEntity` moves `xd` pixels per tick in x
    /// during the launch phase.
    ///
    /// Preconditions: bullet at `(16, 16)` with `xd = 4`, `yd = 0`,
    /// initial `state_count = LAUNCH_START`; fully passable grid so no
    /// removal occurs.
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

    /// Unit under test: `BulletEntity` blocks at a solid background cell
    /// during the launch phase without being removed, so the follow
    /// phase can later snap the projectile back to the player.
    ///
    /// Preconditions: bullet at `(16, 16)` moving right with `xd = 8`;
    /// solid cell at grid cell `(1, 1)` which the bullet would enter
    /// after moving.
    ///
    /// Invariants asserted: after one tick the bullet position is
    /// unchanged and `should_remove` is `false`, mirroring Java
    /// `KniveManager.moveLeftRight` which stops at walls without
    /// killing the entity. The natural `state_count` increment
    /// continues so the projectile transitions into the follow phase
    /// after [`LAUNCH_END`] more ticks.
    #[test]
    fn bullet_blocks_at_solid_cell_during_launch_without_removing() {
        let mut grid = synthetic_grid(64, 64, CellKind::Air);
        set_cell(&mut grid, 1, 1, CellKind::Solid);
        let mut bullet = BulletEntity::with_velocity(16, 16, 8, 8, 8, 0);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        bullet.update(&input, &state, &grid, &mut dispatcher);

        assert_eq!(bullet.x, 16, "bullet must not enter the solid cell");
        assert!(
            !bullet.should_remove(),
            "bullet must stay alive on wall contact during launch so the follow phase can return it",
        );
    }

    /// Unit under test: `BulletEntity::on_kill` snaps the projectile out
    /// of the launch phase and into the follow phase so it starts homing
    /// back to the player after landing a hit on an enemy.
    ///
    /// Preconditions: bullet at the start of the launch phase
    /// (`state_count = LAUNCH_START`).
    ///
    /// Invariants asserted: after `on_kill`, `state_count` equals
    /// `LAUNCH_END + 1`, the first tick of the follow phase.
    #[test]
    fn on_kill_during_launch_starts_follow_phase() {
        let mut bullet = BulletEntity::with_velocity(16, 16, 8, 8, 8, 0);
        assert_eq!(bullet.state_count, LAUNCH_START);
        bullet.on_kill(1, DeathKind::Enemy);
        assert_eq!(
            bullet.state_count,
            LAUNCH_END + 1,
            "on_kill must put the projectile at the first follow-phase tick"
        );
    }

    /// Unit under test: `BulletEntity::observe_player` adjusts horizontal
    /// velocity toward the player during the follow phase.
    ///
    /// Preconditions: bullet at `(100, 50)` moving right with `xd = 8`,
    /// state forced into the follow phase by `on_kill`; player observed
    /// at `(0, 50)` (to the left of the bullet).
    ///
    /// Invariants asserted: after one tick in the follow phase, `xd` has
    /// decremented toward the player and the bullet position has shifted
    /// in that direction.
    #[test]
    fn follow_phase_homes_toward_player() {
        let grid = synthetic_grid(64, 64, CellKind::Air);
        let mut bullet = BulletEntity::with_velocity(100, 50, 8, 8, 8, 0);
        bullet.on_kill(1, DeathKind::Enemy);
        bullet.observe_player(Rect::new(0, 50, 8, 8));

        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        bullet.update(&input, &state, &grid, &mut dispatcher);

        assert_eq!(
            bullet.xd, 7,
            "xd must decrement toward the player on the first follow tick"
        );
        assert_eq!(
            bullet.x, 107,
            "bullet still moves with the freshly adjusted xd"
        );
    }

    /// Unit under test: `BulletEntity::update` removes the projectile
    /// and dispatches `InventoryItem(add Knife)` when the player
    /// catches it during the follow phase.
    ///
    /// Preconditions: bullet at `(20, 20)` forced into the follow phase
    /// by `on_kill`; player bbox observed at an overlapping position.
    ///
    /// Invariants asserted: after one `update`, `should_remove` is
    /// `true` and the captured dispatcher received an
    /// `InventoryItem(add Knife)` message.
    #[test]
    fn player_catches_follow_phase_projectile_returns_knife_to_inventory() {
        use openjill_core::{InventoryObject, MessageHandler, MessageType};
        use std::sync::{Arc, Mutex};

        struct Recorder(Arc<Mutex<Vec<(MessageType, openjill_core::MessagePayload)>>>);
        impl MessageHandler for Recorder {
            fn handle(&mut self, m: MessageType, p: &openjill_core::MessagePayload) {
                self.0.lock().unwrap().push((m, p.clone()));
            }
        }

        let grid = synthetic_grid(64, 64, CellKind::Air);
        let mut bullet = BulletEntity::with_velocity(20, 20, 8, 8, -8, 0);
        bullet.on_kill(1, DeathKind::Enemy);
        bullet.observe_player(Rect::new(16, 16, 16, 16));

        let log: Arc<Mutex<Vec<(MessageType, openjill_core::MessagePayload)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(MessageType::InventoryItem, Box::new(Recorder(log.clone())));

        bullet.update(
            &openjill_core::ActiveInput::new(),
            &RuntimeState::new(),
            &grid,
            &mut dispatcher,
        );

        assert!(
            bullet.should_remove(),
            "follow-phase overlap with the player must be treated as a catch"
        );
        let recorded = log.lock().unwrap().clone();
        let saw_add = recorded.iter().any(|(t, p)| {
            matches!(t, MessageType::InventoryItem)
                && matches!(
                    p,
                    openjill_core::MessagePayload::InventoryItem(payload)
                        if payload.item == InventoryObject::Knife && payload.add
                )
        });
        assert!(
            saw_add,
            "catch must dispatch InventoryItem(add Knife) so the player gets the knife back"
        );
    }

    /// Unit under test: a knife that lands on a floor (fall phase
    /// `overlaps_solid` true) snaps back to [`NO_MOVE_NO_HIT`] and
    /// stays alive as a pickup instead of being removed.
    ///
    /// Preconditions: bullet forced into the fall phase
    /// (`state_count = FOLLOW_MAX + 1`) directly above a solid cell.
    ///
    /// Invariants asserted: after one `update`, `state_count` is
    /// `NO_MOVE_NO_HIT` and `should_remove` is `false`.
    #[test]
    fn fall_onto_floor_snaps_to_pickup_state() {
        let mut grid = synthetic_grid(64, 64, CellKind::Air);
        // Solid cell at column 2, row 3 covers pixels [32, 48) x [48, 64).
        // Bullet placed at y = 47 so one fall step (FALL_STEP_Y = 1)
        // pushes it onto the solid row.
        set_cell(&mut grid, 2, 3, CellKind::Solid);
        let mut bullet = BulletEntity::with_velocity(32, 47, 8, 8, 0, 0);
        bullet.state_count = FOLLOW_MAX + 1;
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        bullet.update(&input, &state, &grid, &mut dispatcher);
        assert_eq!(bullet.state_count, NO_MOVE_NO_HIT);
        assert!(
            !bullet.should_remove(),
            "landed knife must stay alive as a recoverable pickup"
        );
    }

    /// Unit under test: a knife in the pickup (`NO_MOVE_NO_HIT`) state
    /// dispatches `InventoryItem(add Knife)` and self-removes when the
    /// player walks onto it.
    #[test]
    fn landed_knife_picks_up_on_player_overlap() {
        use openjill_core::{InventoryObject, MessageHandler, MessageType};
        use std::sync::{Arc, Mutex};

        struct Recorder(Arc<Mutex<Vec<(MessageType, openjill_core::MessagePayload)>>>);
        impl MessageHandler for Recorder {
            fn handle(&mut self, m: MessageType, p: &openjill_core::MessagePayload) {
                self.0.lock().unwrap().push((m, p.clone()));
            }
        }

        let grid = synthetic_grid(64, 64, CellKind::Air);
        let mut bullet = BulletEntity::with_velocity(40, 40, 8, 8, 0, 0);
        bullet.state_count = NO_MOVE_NO_HIT;
        bullet.observe_player(Rect::new(36, 36, 16, 16));

        let log: Arc<Mutex<Vec<(MessageType, openjill_core::MessagePayload)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(MessageType::InventoryItem, Box::new(Recorder(log.clone())));

        bullet.update(
            &openjill_core::ActiveInput::new(),
            &RuntimeState::new(),
            &grid,
            &mut dispatcher,
        );

        assert!(bullet.should_remove(), "pickup overlap must remove knife");
        let recorded = log.lock().unwrap().clone();
        assert!(recorded.iter().any(|(t, p)| {
            matches!(t, MessageType::InventoryItem)
                && matches!(
                    p,
                    openjill_core::MessagePayload::InventoryItem(payload)
                        if payload.item == InventoryObject::Knife && payload.add
                )
        }));
    }

    /// Unit under test: a knife that would step out of the map during
    /// the launch phase stops at the edge instead of being removed,
    /// so the follow phase can still pull it back.
    #[test]
    fn launch_off_map_clamps_instead_of_removing() {
        let grid = synthetic_grid(8, 8, CellKind::Air);
        // `BulletEntity::in_map` uses the constant
        // BACKGROUND_GRID_WIDTH * BLOCK_SIZE_I = 2048 px. Place the
        // bullet just inside the right edge so one tick at xd = 16
        // pushes it past 2048; it should slide flush to the edge
        // (2048 - width = 2032) rather than overrun or be removed.
        let mut bullet = BulletEntity::with_velocity(2030, 32, 16, 16, 16, 0);
        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        bullet.update(&input, &state, &grid, &mut dispatcher);
        assert_eq!(bullet.x, 2032, "launch must slide flush to the map edge");
        assert!(
            !bullet.should_remove(),
            "off-map launch clamp must not remove the projectile"
        );
    }

    /// Unit under test: the follow phase resolves X and Y independently so a
    /// returning boomerang slides flush along a wall instead of wedging.
    ///
    /// Regression: the old combined all-or-nothing step froze BOTH axes when
    /// the combined destination overlapped a wall, stranding the knife. Java
    /// `KniveManager.followPlayer` moves left/right and up/down separately.
    #[test]
    fn follow_slides_each_axis_independently_against_wall() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        // Solid wall column at cell x = 1 (pixels 16..32), full height.
        for cy in 0..8 {
            set_cell(&mut grid, 1, cy, CellKind::Solid);
        }
        // 8x8 knife just right of the wall, homing up-left toward the player.
        let mut bullet = BulletEntity::with_velocity(34, 40, 8, 8, -8, -4);
        bullet.state_count = LAUNCH_END + 1; // follow phase
        bullet.observe_player(Rect::new(0, 0, 8, 8));

        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        bullet.update(&input, &state, &grid, &mut dispatcher);

        // X slides flush to the wall's right edge (32) instead of refusing; Y
        // still advances upward by the full step (40 - 4 = 36).
        assert_eq!(bullet.x, 32, "knife slides flush against the wall");
        assert_eq!(bullet.y, 36, "knife still climbs on the unblocked Y axis");
        assert!(!bullet.should_remove(), "knife must not be lost");
    }

    /// Regression: a knife thrown while flush against a wall spawns embedded in
    /// the wall cell; the follow phase must still slide it out toward the
    /// player rather than wedging it permanently (leading-edge wall test).
    #[test]
    fn follow_unwedges_knife_spawned_inside_wall() {
        let mut grid = synthetic_grid(8, 8, CellKind::Air);
        // Solid wall column at cell x = 2 (pixels 32..48), full height.
        for cy in 0..8 {
            set_cell(&mut grid, 2, cy, CellKind::Solid);
        }
        // 10x10 knife spawned at the wall face (x = 32), fully inside cell 2,
        // homing left toward the player.
        let mut bullet = BulletEntity::with_velocity(32, 40, 10, 10, -8, 0);
        bullet.state_count = LAUNCH_END + 1; // follow phase
        bullet.observe_player(Rect::new(0, 40, 16, 16));
        let x0 = bullet.x;

        let input = openjill_core::ActiveInput::new();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();
        bullet.update(&input, &state, &grid, &mut dispatcher);

        assert!(
            bullet.x < x0,
            "knife slides left out of the wall (was wedged at {x0}, now {})",
            bullet.x
        );
        assert!(!bullet.should_remove(), "knife must not be lost");
    }
}
