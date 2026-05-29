//! Snake enemy entity (JN object type 39).
//!
//! Mirrors `org.jill.game.entities.obj.SnakeManager`: a horizontal floor
//! patroller drawn as a head, a run of middle body segments, and a tail.  It
//! is **multi-hit**: each weapon hit (outside the brief recoil window) removes
//! one middle segment (`width -= middleWidth`) and starts a `stateWhenTouch`
//! recoil cooldown during which it cannot be hit again; it dies only once the
//! body has shrunk to just head + tail.
//!
//! Tileset/tile from `object_conf.json` (`tileSet = 15`): head tiles are
//! 16x16, tail/middle tiles 8x4.  `rightTileHead = 0,1,0,2`,
//! `rightTileTail = 7,7,8,8`, `leftTileHead = 9,10,9,11`,
//! `leftTileTail = 12,12,13,13`, `middleTile = 3,4,5,6`, `stateWhenTouch = 16`.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_under_next};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the snake frames (`tileSet = 15`).
const TILESET_INDEX: u8 = 15;
/// Animation frames per body part.
const FRAMES: usize = 4;
/// Head tile width/height in pixels (tiles 0-2, 9-11 are 16x16).
const HEAD_W: i32 = 16;
const HEAD_H: i32 = 16;
/// Tail / middle segment width and height (tiles 3-8, 12-13 are 8x4).
const TAIL_W: i32 = 8;
const MIDDLE_W: i32 = 8;
const BODY_H: i32 = 4;
/// Horizontal patrol speed in pixels per tick.
const X_SPEED: i32 = 3;
/// Score awarded when the snake finally dies (`point = 35`).
const SCORE_VALUE: i32 = 100;
/// Recoil cooldown after a hit (`stateWhenTouch = 16`); the snake is immune
/// while it counts back down to zero.
const STATE_WHEN_TOUCH: i32 = 16;

/// Head animation tiles, right-facing (`rightTileHead = 0,1,0,2`).
const RIGHT_HEAD: [u16; FRAMES] = [0, 1, 0, 2];
/// Tail animation tiles, right-facing (`rightTileTail = 7,7,8,8`).
const RIGHT_TAIL: [u16; FRAMES] = [7, 7, 8, 8];
/// Head animation tiles, left-facing (`leftTileHead = 9,10,9,11`).
const LEFT_HEAD: [u16; FRAMES] = [9, 10, 9, 11];
/// Tail animation tiles, left-facing (`leftTileTail = 12,12,13,13`).
const LEFT_TAIL: [u16; FRAMES] = [12, 12, 13, 13];
/// Middle body animation tiles (`middleTile = 3,4,5,6`).
const MIDDLE: [u16; FRAMES] = [3, 4, 5, 6];

pub struct SnakeEntity {
    x: i32,
    y: i32,
    /// Current total width in pixels; shrinks by [`MIDDLE_W`] per hit.
    w: i32,
    h: i32,
    x_speed: i32,
    counter: usize,
    /// Recoil cooldown; the snake is immune to weapons while `> 0`.
    recoil: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
}

impl SnakeEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(TILESET_INDEX, 14, "SnakeEntity tiles");
        // The snake's length comes from its JN record width; fall back to a
        // minimal head + one segment + tail body when the record carries none.
        let min_w = HEAD_W + MIDDLE_W + TAIL_W;
        let w = i32::from(item.width()).max(min_w);
        let h = i32::from(item.height()).max(HEAD_H);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed: X_SPEED,
            counter: 0,
            recoil: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
        }
    }

    /// Y offset of the thin body row (head occupies the full height, the
    /// tail/middle segments sit along the bottom: Java `tailY = height - 4`).
    fn body_y(&self) -> i32 {
        self.y + self.h - BODY_H
    }
}

impl ObjectEntity for SnakeEntity {
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

        // Floor patrol: advance and animate, or reverse at a wall/gap.
        if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
        {
            self.x += self.x_speed;
            self.counter = (self.counter + 1) % FRAMES;
        } else {
            self.x_speed = -self.x_speed;
            self.counter = 0;
        }

        // Recoil cooldown counts back down (Java `if state > 0 setState(state - 1)`).
        if self.recoil > 0 {
            self.recoil -= 1;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        // The snake is composed of several tiles; see `draw_multi`.  `draw`
        // returns the head so single-sprite callers still get something.
        if self.dead {
            return None;
        }
        let head = if self.x_speed < 0 {
            LEFT_HEAD[self.counter]
        } else {
            RIGHT_HEAD[self.counter]
        };
        let head_x = if self.x_speed < 0 {
            self.x
        } else {
            self.x + self.w - HEAD_W
        };
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: head,
            x: head_x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn draw_multi(&self) -> Vec<RenderCommand> {
        if self.dead {
            return vec![];
        }
        let c = self.counter;
        let body_y = self.body_y();
        let mut cmds = Vec::new();
        let blit = |tile: u16, x: i32, y: i32| RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile,
            x,
            y,
            opaque: false,
            clip: None,
        };

        if self.x_speed < 0 {
            // Head on the left, tail on the right.
            cmds.push(blit(LEFT_HEAD[c], self.x, self.y));
            let tail_rel = self.w - TAIL_W;
            let mut dx = HEAD_W;
            while dx < tail_rel {
                cmds.push(blit(MIDDLE[c], self.x + dx, body_y));
                dx += MIDDLE_W;
            }
            cmds.push(blit(LEFT_TAIL[c], self.x + tail_rel, body_y));
        } else {
            // Head on the right, tail on the left.
            let head_rel = self.w - HEAD_W;
            cmds.push(blit(RIGHT_TAIL[c], self.x, body_y));
            let mut dx = TAIL_W;
            while dx < head_rel {
                cmds.push(blit(MIDDLE[c], self.x + dx, body_y));
                dx += MIDDLE_W;
            }
            cmds.push(blit(RIGHT_HEAD[c], self.x + head_rel, self.y));
        }
        cmds
    }

    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.dead || self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
        self.pending_kill = Some(DeathKind::Enemy);
    }

    /// A weapon hit removes one middle segment and starts a recoil window;
    /// the snake dies only once its body is down to head + tail
    /// (Java `SnakeManager.msgKill`).
    fn on_kill(&mut self, damage: i32, _death_kind: DeathKind) {
        if self.dead || damage < 1 || self.recoil > 0 {
            return;
        }
        self.recoil = STATE_WHEN_TOUCH;
        self.w -= MIDDLE_W;
        if self.w <= TAIL_W + HEAD_W {
            self.dead = true;
        }
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn is_dead(&self) -> bool {
        self.dead
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snake_with_width(w: i32) -> SnakeEntity {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 39; // snake
        let jn = openjill_data::jn::JnFile::from_bytes(bytes).expect("synthetic JN parses");
        let cache = AssetCache::synthetic();
        let mut snake = SnakeEntity::new(&jn.objects()[0], &cache);
        snake.w = w;
        snake.h = HEAD_H;
        snake
    }

    fn expire_recoil(snake: &mut SnakeEntity) {
        let grid = BackgroundGrid::new(Vec::new());
        for _ in 0..STATE_WHEN_TOUCH {
            snake.update(
                &ActiveInput::default(),
                &RuntimeState::new(),
                &grid,
                &mut MessageDispatcher::new(),
            );
        }
    }

    /// Each hit removes one middle segment; the snake survives until only the
    /// head + tail remain, and is immune during the recoil window.
    #[test]
    fn snake_takes_multiple_hits_to_die() {
        // 16 + 3*8 + 8 = 48 px: three middle segments.
        let mut snake = snake_with_width(HEAD_W + 3 * MIDDLE_W + TAIL_W);

        for hit in 0..3 {
            assert!(!snake.dead, "snake alive before hit {hit}");
            let w_before = snake.w;
            snake.on_kill(1, DeathKind::Enemy);
            assert_eq!(snake.w, w_before - MIDDLE_W, "one segment removed per hit");

            // Immune during recoil: an immediate second hit does nothing.
            let w_after = snake.w;
            snake.on_kill(1, DeathKind::Enemy);
            assert_eq!(snake.w, w_after, "snake immune during recoil");

            expire_recoil(&mut snake);
        }

        // After three segment hits the body is head + tail -> dead.
        assert!(snake.dead, "snake dies once shrunk to head + tail");
    }

    /// The bounding box shrinks as segments are removed.
    #[test]
    fn snake_bounding_box_shrinks_with_hits() {
        let mut snake = snake_with_width(HEAD_W + 4 * MIDDLE_W + TAIL_W);
        let w0 = snake.bounding_box().w;
        snake.on_kill(1, DeathKind::Enemy);
        assert!(snake.bounding_box().w < w0, "bbox shrinks after a hit");
    }
}
