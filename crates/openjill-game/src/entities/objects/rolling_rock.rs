//! Rolling rock hazard entity (JN object type 35).
//!
//! Mirrors `org.jill.game.entities.obj.RollingRockManager` from the Java
//! reference: the rock moves horizontally along a floor, reversing direction
//! at walls or gaps, animates through four frames as it rolls, and kills the
//! player on contact.
//!
//! Per-tick logic follows the Java reference's `msgUpdate`:
//!
//! 1. Try to advance horizontally by [`X_SPEED_FALL`] in the current
//!    direction.  When the next column is blocked or the floor under the next
//!    position is missing, reverse direction so the rock keeps rolling on the
//!    visible platform.
//! 2. Advance the animation counter through frames `1..=COUNTER_MOVE_MAX`;
//!    `counter = COUNTER_FALL = 0` is reserved for the airborne sprite.
//! 3. The current port keeps the rock pinned to a one-cell floor: it does not
//!    yet simulate the full Java fall arc (`ySpeed += ySpeedFall` ramp); when
//!    that lands enemies in #62 share the same fall arithmetic.
//!
//! Tile / tileset identity comes from `object_conf.json`: `tileSet = 14`,
//! `tile = 24`, `numberTileSet = 4`, `counterFall = 0`, `counterMoveMax = 3`,
//! `counterMoveValue = 1`, `xSpeedFall = 4`.
//!
//! FIXME(epic-6): confirm tileset 14 tiles 24..=27 against the original
//! `JILL1.SHA` once the SHA dumper grows a visual / PNG output.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset that owns the rolling-rock frames.
///
/// REVERSE-ENGINEERED: `RollingRockManager.tileSet = 14` in
/// `object_conf.json`. Tileset 14 is multi-purpose (45 tiles total, shared
/// with falling spikes, keys, etc.); rolling rock uses tiles 24-27. Not
/// derivable from SHA structure; future engine config file should expose
/// this.
const TILESET_INDEX: u8 = 14;

/// Base tile index for the four rolling-rock animation frames.
///
/// REVERSE-ENGINEERED: `RollingRockManager.tile = 24` in `object_conf.json`.
const TILE_BASE: u16 = 24;

/// Number of distinct frames in the animation.
///
/// REVERSE-ENGINEERED: `RollingRockManager.numberTileSet = 4` in
/// `object_conf.json`. Verified at construction by
/// [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: u16 = 4;

/// Counter slot reserved for the airborne / falling sprite.
///
/// REVERSE-ENGINEERED: `RollingRockManager.counterFall = 0` in
/// `object_conf.json`.
const COUNTER_FALL: i32 = 0;

/// Rolling animation counter cap.
///
/// REVERSE-ENGINEERED: `RollingRockManager.counterMoveMax = 3` in
/// `object_conf.json`.
const COUNTER_MOVE_MAX: i32 = 3;

/// Counter value the animation wraps to after exceeding [`COUNTER_MOVE_MAX`].
///
/// REVERSE-ENGINEERED: `RollingRockManager.counterMoveValue = 1` in
/// `object_conf.json`.
const COUNTER_MOVE_VALUE: i32 = 1;

/// Per-tick horizontal motion in pixels.
///
/// REVERSE-ENGINEERED: `RollingRockManager.xSpeedFall = 4` in
/// `object_conf.json`.
const X_SPEED_FALL: i32 = 4;

/// Rolling rock entity.
pub struct RollingRockEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Current horizontal velocity in pixels per tick; positive = right,
    /// negative = left.  Initialised to [`X_SPEED_FALL`] so the rock starts
    /// rolling immediately, matching the Java reference's
    /// `setxSpeed(xSpeedFall)` path on the first floor-supported tick.
    x_speed: i32,
    /// Animation counter; `0` is the airborne sprite, `1..=COUNTER_MOVE_MAX`
    /// the four rolling frames.
    counter: i32,
    /// Pending player-kill classification armed in [`Self::on_touch`] and
    /// drained by the level loop via [`ObjectEntity::take_player_kill`].
    pending_kill: Option<DeathKind>,
}

impl RollingRockEntity {
    /// Builds a rolling rock from a JN object record.
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "RollingRockEntity NUMBER_TILE_SET",
        );
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed: X_SPEED_FALL,
            counter: COUNTER_MOVE_VALUE,
            pending_kill: None,
        }
    }

    /// Returns `true` when the cell column ahead of the rock at the rock's
    /// vertical level overlaps a solid cell.  Mirrors
    /// `UtilityObjectEntity.giveBlockAtRight` / `giveBlockAtLeft`.
    fn blocked_ahead(&self, backgrounds: &BackgroundGrid) -> bool {
        let probe_x = if self.x_speed > 0 {
            self.x + self.w
        } else {
            self.x - 1
        };
        let cell_x = probe_x.div_euclid(BLOCK_SIZE_I);
        if cell_x < 0 || cell_x as usize >= backgrounds.width {
            return true;
        }
        let cell_x = cell_x as usize;
        let cell_y_top = self.y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cell_y_bot = (self.y + self.h - 1).div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cell_y_bot = cell_y_bot.min(backgrounds.height.saturating_sub(1));
        for cy in cell_y_top..=cell_y_bot {
            if let Some(cell) = backgrounds.get(cell_x, cy)
                && (!cell.is_passthrough() || cell.is_stair())
            {
                return true;
            }
        }
        false
    }

    /// Returns `true` when the cell one row below the rock's next position is
    /// solid (the rock keeps rolling).  Returns `false` when the column ahead
    /// has no floor (the rock would walk off a cliff), which triggers the
    /// reverse-direction branch.
    fn floor_under_next(&self, backgrounds: &BackgroundGrid) -> bool {
        let probe_x = if self.x_speed > 0 {
            self.x + self.w
        } else {
            self.x - 1
        };
        let cell_x = probe_x.div_euclid(BLOCK_SIZE_I);
        if cell_x < 0 || cell_x as usize >= backgrounds.width {
            return false;
        }
        let cell_x = cell_x as usize;
        let cell_y = (self.y + self.h).div_euclid(BLOCK_SIZE_I);
        if cell_y < 0 || cell_y as usize >= backgrounds.height {
            return false;
        }
        let cell_y = cell_y as usize;
        let Some(cell) = backgrounds.get(cell_x, cell_y) else {
            return false;
        };
        !cell.is_passthrough() || cell.is_stair()
    }
}

impl ObjectEntity for RollingRockEntity {
    /// Advances the rock by one tick.  When the path ahead is open and the
    /// floor under the next position holds, the rock advances by
    /// [`X_SPEED_FALL`] and steps the animation counter through
    /// `1..=COUNTER_MOVE_MAX`; otherwise it reverses horizontal direction so
    /// the next tick rolls the rock the other way.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        let blocked = self.blocked_ahead(backgrounds);
        let supported = self.floor_under_next(backgrounds);
        if !blocked && supported {
            self.x += self.x_speed;
            if self.counter != COUNTER_FALL {
                self.counter += 1;
                if self.counter > COUNTER_MOVE_MAX {
                    self.counter = COUNTER_MOVE_VALUE;
                }
            }
        } else {
            self.x_speed = -self.x_speed;
        }
    }

    /// Emits a [`RenderCommand::Blit`] for the current animation frame.
    fn draw(&self) -> Option<RenderCommand> {
        let frame = self.counter.clamp(0, (NUMBER_TILE_SET as i32) - 1) as u16;
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: TILE_BASE + frame,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Arms a [`DeathKind::OtherBackground`] kill that the level loop drains
    /// via [`Self::take_player_kill`] and applies to the player.  No direct
    /// `DieRestartLevel` dispatch: the player's `Die` sub-state fires it once
    /// the die animation finishes.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        self.pending_kill = Some(DeathKind::OtherBackground);
    }

    /// Rolling rocks are indestructible; weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the entity's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns the pending kill classification (and clears it) so the level
    /// loop can apply it to the player after the touch dispatch pass.
    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::{RollingRockEntity, TILE_BASE, TILESET_INDEX, X_SPEED_FALL};
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        ActiveInput, BackgroundEntity, BackgroundGrid, MessageDispatcher, ObjectEntity,
        RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;

    const OBJECT_RECORD_BYTES: usize = 31;

    struct SolidIf {
        solid: bool,
    }

    impl BackgroundEntity for SolidIf {
        fn draw(&self, _x: i32, _y: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _cx: i32, _cy: i32, _dispatcher: &mut MessageDispatcher) {}
        fn on_player_touch(
            &mut self,
            _player: &mut dyn ObjectEntity,
            _dispatcher: &mut MessageDispatcher,
        ) {
        }
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

    /// Builds a grid where row `floor_row` is entirely solid and everything
    /// else is open air.  Use this to give the rock something to roll on.
    fn grid_with_floor_row(width: usize, height: usize, floor_row: usize) -> BackgroundGrid {
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

    /// Replaces cell `(x, y)` with a wall (or removes the floor) for the
    /// reverse-direction test.
    fn set_solid(grid: &mut BackgroundGrid, x: usize, y: usize, solid: bool) {
        grid.cells[y][x] = Box::new(SolidIf { solid });
    }

    fn synthetic_rock_at(x: i32, y: i32) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 35; // object_type = RollingRockManager
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`RollingRockEntity::update`] reverses direction when
    /// the next column is a wall.
    ///
    /// Preconditions: a rock at world `(32, 32)` sits on a solid floor row at
    /// `y = 48`; the column at `x = 48` (one cell to the right of the rock)
    /// is solid from `y = 32` upward, simulating a wall.
    ///
    /// Invariants asserted: after one tick the rock has not moved (blocked
    /// from advancing) but its `x_speed` has flipped sign, so the following
    /// tick rolls it left.
    #[test]
    fn rolling_rock_reverses_at_wall() {
        let cache = AssetCache::synthetic();
        let mut rock = RollingRockEntity::new(&synthetic_rock_at(32, 32), &cache);
        let mut backgrounds = grid_with_floor_row(8, 6, 3);
        // Place a wall one column to the right of the rock.
        set_solid(&mut backgrounds, 3, 2, true);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        rock.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(rock.bounding_box().x, 32, "rock stays put on blocked tick");

        // Removing the wall lets the rock move left now that its direction
        // has flipped.
        set_solid(&mut backgrounds, 3, 2, false);
        rock.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            rock.bounding_box().x,
            32 - X_SPEED_FALL,
            "after reverse, rock rolls left by X_SPEED_FALL"
        );
    }

    /// Unit under test: [`RollingRockEntity::update`] reverses direction when
    /// the floor under the next position is missing (gap).
    ///
    /// Preconditions: a rock at `(32, 32)` sits on a solid floor row at
    /// `y = 48` *except* the cell immediately right of the rock has no
    /// floor.  The rock cannot step into the gap.
    ///
    /// Invariants asserted: after one tick the rock has flipped direction; the
    /// next tick advances it left across the existing floor.
    #[test]
    fn rolling_rock_reverses_at_gap() {
        let cache = AssetCache::synthetic();
        let mut rock = RollingRockEntity::new(&synthetic_rock_at(32, 32), &cache);
        let mut backgrounds = grid_with_floor_row(8, 6, 3);
        // Drop the floor under the next column to create a gap.
        set_solid(&mut backgrounds, 3, 3, false);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        rock.update(&input, &state, &backgrounds, &mut dispatcher);
        // Restore the floor so the post-reverse tick has somewhere to land.
        set_solid(&mut backgrounds, 3, 3, true);
        rock.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            rock.bounding_box().x,
            32 - X_SPEED_FALL,
            "after gap reverse, rock rolls left by X_SPEED_FALL"
        );
    }

    /// Unit under test: [`RollingRockEntity::draw`] selects a tile in the
    /// SHA-verified animation range.
    #[test]
    fn rolling_rock_draw_targets_verified_sha_range() {
        let cache = AssetCache::synthetic();
        let rock = RollingRockEntity::new(&synthetic_rock_at(0, 0), &cache);
        match rock.draw().expect("rock must draw") {
            RenderCommand::Blit { tileset, tile, .. } => {
                assert_eq!(tileset, TILESET_INDEX);
                assert!(
                    (TILE_BASE..TILE_BASE + 4).contains(&tile),
                    "rolling rock tile must fall in {TILE_BASE}..{}",
                    TILE_BASE + 4
                );
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }
}
