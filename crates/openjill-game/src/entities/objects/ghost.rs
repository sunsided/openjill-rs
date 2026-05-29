//! Ghost enemy entity (JN object type 53).
//!
//! Mirrors `org.jill.game.entities.obj.GhostManager`: the ghost glides through
//! a corridor of cells that share one background map code (typically open air,
//! map code 0), turning at the corridor's boundaries.  It moves either
//! vertically or horizontally; on reaching a cell whose map code differs it
//! stops that axis and turns onto a perpendicular branch whose neighbouring
//! cell still matches the corridor.  The travel speed magnitude is the JN
//! record's `counter` field.
//!
//! Tileset from `object_conf.json`: `tileSet = 50`, `numberTileSet = 4` -
//! four directional frames: right = 0, left = 1, up = 2, down = 3.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundEntity, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload,
    MessageType, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset carrying the ghost frames (`tileSet = 50`).
const TILESET_INDEX: u8 = 50;
/// Directional frame indices (`IMAGE_RIGHT/LEFT/UP/DOWN`).
const TILE_RIGHT: u16 = 0;
const TILE_LEFT: u16 = 1;
const TILE_UP: u16 = 2;
const TILE_DOWN: u16 = 3;
/// Score awarded when the ghost is killed.
const SCORE_VALUE: i32 = 300;
/// Travel speed used when the JN record carries no `counter`.
const DEFAULT_SPEED: i32 = 2;

/// Returns the background map code at cell `(cx, cy)`, or `None` when the cell
/// is outside the grid.  Cells with no DMA entry (open air) report code `0`,
/// matching the Java reference's fully-populated background array.
fn cell_code(backgrounds: &BackgroundGrid, cx: i32, cy: i32) -> Option<u16> {
    if cx < 0 || cy < 0 {
        return None;
    }
    let (cx, cy) = (cx as usize, cy as usize);
    if cx >= backgrounds.width || cy >= backgrounds.height {
        return None;
    }
    Some(
        backgrounds
            .get(cx, cy)
            .and_then(BackgroundEntity::dma_map_code)
            .unwrap_or(0),
    )
}

pub struct GhostEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    y_speed: i32,
    /// Travel speed magnitude (JN `counter`).
    speed: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live state written back.
    origin: JnObject,
}

impl GhostEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(TILESET_INDEX, 4, "GhostEntity tiles");
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let speed = {
            let c = i32::from(item.counter()).abs();
            if c != 0 { c } else { DEFAULT_SPEED }
        };
        let xs = i32::from(item.x_speed());
        let ys = i32::from(item.y_speed());
        // Start gliding right when the record specifies no initial direction.
        let (x_speed, y_speed) = if xs == 0 && ys == 0 {
            (speed, 0)
        } else {
            (xs, ys)
        };
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed,
            y_speed,
            speed,
            dead: false,
            score_dispatched: false,
            zaphold: i32::from(item.zap_hold()),
            pending_kill: None,
            origin: item.clone(),
        }
    }

    /// Vertical glide (Java `moveUpDown`): continue while the destination cell
    /// shares the corridor map code, otherwise stop and turn left/right onto a
    /// matching branch.
    fn move_up_down(&mut self, backgrounds: &BackgroundGrid) {
        let block_x = self.x.div_euclid(BLOCK_SIZE_I);
        let new_y = self.y + self.y_speed;
        let (block_y, new_block_y) = if self.y_speed > 0 {
            self.y_speed = self.speed;
            (
                (self.y + self.h - 1).div_euclid(BLOCK_SIZE_I),
                (new_y + self.h - 1).div_euclid(BLOCK_SIZE_I),
            )
        } else {
            self.y_speed = -self.speed;
            (
                self.y.div_euclid(BLOCK_SIZE_I),
                new_y.div_euclid(BLOCK_SIZE_I),
            )
        };

        let current = cell_code(backgrounds, block_x, block_y);
        if current.is_some() && current == cell_code(backgrounds, block_x, new_block_y) {
            self.y = new_y;
        } else {
            self.y_speed = 0;
            if current == cell_code(backgrounds, block_x + 1, block_y) {
                self.x_speed = self.speed; // turn right
            } else if current == cell_code(backgrounds, block_x - 1, block_y) {
                self.x_speed = -self.speed; // turn left
            }
        }
    }

    /// Horizontal glide (Java `moveLeftRight`).
    fn move_left_right(&mut self, backgrounds: &BackgroundGrid) {
        let block_y = self.y.div_euclid(BLOCK_SIZE_I);
        let new_x = self.x + self.x_speed;
        let (block_x, new_block_x) = if self.x_speed > 0 {
            (
                (self.x + self.w - 1).div_euclid(BLOCK_SIZE_I),
                (new_x + self.w - 1).div_euclid(BLOCK_SIZE_I),
            )
        } else {
            (
                self.x.div_euclid(BLOCK_SIZE_I),
                new_x.div_euclid(BLOCK_SIZE_I),
            )
        };

        let current = cell_code(backgrounds, block_x, block_y);
        if current.is_some() && current == cell_code(backgrounds, new_block_x, block_y) {
            self.x = new_x;
        } else {
            self.x_speed = 0;
            if current == cell_code(backgrounds, block_x, block_y + 1) {
                self.y_speed = self.speed; // turn down
            } else if current == cell_code(backgrounds, block_x, block_y - 1) {
                self.y_speed = -self.speed; // turn up
            }
        }
    }
}

impl ObjectEntity for GhostEntity {
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

        if self.y_speed != 0 {
            self.move_up_down(backgrounds);
        }
        if self.x_speed != 0 {
            self.move_left_right(backgrounds);
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.dead {
            return None;
        }
        let tile = if self.x_speed < 0 {
            TILE_LEFT
        } else if self.x_speed > 0 {
            TILE_RIGHT
        } else if self.y_speed > 0 {
            TILE_DOWN
        } else {
            TILE_UP
        };
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile,
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

    /// Snapshots the live ghost for a save game, or `None` once dead.
    ///
    /// Persists position, the live glide velocity, and `zap_hold`; the speed
    /// magnitude lives in the authored `counter` (preserved from the origin).
    fn snapshot(&self) -> Option<JnObject> {
        if self.dead {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        // `new()` maps an authored zero velocity to `(speed, 0)`; emit the
        // authored `(0, 0)` when the live velocity is exactly that default so
        // the round-trip stays exact, and the live velocity otherwise.
        let (xs, ys) = if self.x_speed == self.speed
            && self.y_speed == 0
            && obj.x_speed() == 0
            && obj.y_speed() == 0
        {
            (obj.x_speed(), obj.y_speed())
        } else {
            (self.x_speed as i16, self.y_speed as i16)
        };
        obj.set_speed(xs, ys);
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

    /// Background cell carrying an explicit DMA map code.
    struct CodeCell(u16);
    impl BackgroundEntity for CodeCell {
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
        fn dma_map_code(&self) -> Option<u16> {
            Some(self.0)
        }
    }

    /// Builds a grid where code `0` is the corridor and code `1` is a wall.
    fn grid(w: usize, h: usize, f: impl Fn(usize, usize) -> u16) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(h);
        for y in 0..h {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(w);
            for x in 0..w {
                row.push(Box::new(CodeCell(f(x, y))));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn ghost(x: i32, y: i32, xs: i32, ys: i32) -> GhostEntity {
        GhostEntity {
            x,
            y,
            w: 16,
            h: 16,
            x_speed: xs,
            y_speed: ys,
            speed: 2,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
            origin: openjill_data::jn::JnObject::spawned(53, x as u16, y as u16, 16, 16),
        }
    }

    fn tick(g: &mut GhostEntity, bg: &BackgroundGrid) {
        g.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            bg,
            &mut MessageDispatcher::new(),
        );
    }

    /// A ghost gliding right along a code-0 corridor turns down when the
    /// corridor ends at a wall and a code-0 cell sits below.
    #[test]
    fn ghost_follows_corridor_and_turns_at_wall() {
        // Row 1 is the corridor (code 0) for columns 0..=2; column 3 is a wall
        // (code 1). Below the corridor end (col 2, row 2) is also code 0.
        let bg = grid(5, 5, |x, y| {
            if x == 3 {
                1
            } else if y == 1 || (x == 2 && y >= 1) {
                0
            } else {
                1
            }
        });
        // Ghost moving right in the corridor at (16,16) (cell 1,1).
        let mut g = ghost(16, 16, 2, 0);
        let start_x = g.x;
        for _ in 0..40 {
            tick(&mut g, &bg);
        }
        // It advanced right, then on hitting the wall turned downward.
        assert!(g.x > start_x, "ghost glided along the corridor");
        assert!(g.y > 16, "ghost turned down at the corridor boundary");
    }
}
