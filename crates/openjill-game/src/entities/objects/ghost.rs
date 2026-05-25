//! Ghost enemy entity (JN object type 53).
//!
//! Mirrors `org.jill.game.entities.obj.GhostManager`: vertical sine-wave
//! movement; travels horizontally with a vertical oscillation; reverses
//! horizontal component on wall collision; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 50`, `tile = 0`,
//! `numberTileSet = 2` (tiles 0-1 = ghost animation frames).

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset entry carrying ghost sprite frames.
///
/// REVERSE-ENGINEERED: `GhostManager.tileSet = 50` in `object_conf.json`.
/// Not derivable from SHA structure; future engine config file should
/// expose this.
const TILESET_INDEX: u8 = 50;
/// Base tile index within [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED: `GhostManager.tile = 0` in `object_conf.json`.
const TILE_BASE: u16 = 0;
/// Number of animation frames cycled by the ghost sprite.
///
/// REVERSE-ENGINEERED: ghost animates 2 visible frames; tileset 50 carries
/// 4 tiles total (verified at construction by
/// [`AssetCache::assert_tile_subset`]). The Java reference's
/// `GhostManager.numberTileSet = 4` value reflects the full tileset, but
/// the Rust port animates only the first two frames to match observed
/// in-game behaviour.
const NUMBER_TILE_SET: u16 = 2;
const SCORE_VALUE: i32 = 300;
const DEFAULT_X_SPEED: i32 = 3;
/// Amplitude of vertical oscillation in pixels.
const OSCILLATION_AMPLITUDE: i32 = 2;
/// Ticks per oscillation half-cycle.
const OSCILLATION_HALF_PERIOD: i32 = 8;

pub struct GhostEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    oscillation_tick: i32,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
}

impl GhostEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "GhostEntity NUMBER_TILE_SET",
        );
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let xd = i32::from(item.x_speed());
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed: if xd != 0 { xd } else { DEFAULT_X_SPEED },
            oscillation_tick: 0,
            counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
        }
    }

    fn blocked_horizontal(&self, backgrounds: &BackgroundGrid, nx: i32) -> bool {
        let cx = if self.x_speed > 0 {
            (nx + self.w - 1).div_euclid(BLOCK_SIZE_I)
        } else {
            nx.div_euclid(BLOCK_SIZE_I)
        };
        if cx < 0 || cx as usize >= backgrounds.width {
            return true;
        }
        let cx = cx as usize;
        let cy_t = self.y.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cy_b = (self.y + self.h - 1)
            .div_euclid(BLOCK_SIZE_I)
            .max(0)
            .min((backgrounds.height as i32) - 1) as usize;
        for cy in cy_t..=cy_b {
            if let Some(cell) = backgrounds.get(cx, cy)
                && (!cell.is_passthrough() || cell.is_stair())
            {
                return true;
            }
        }
        false
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

        // Horizontal with wall bounce.
        let nx = self.x + self.x_speed;
        if self.blocked_horizontal(backgrounds, nx) {
            self.x_speed = -self.x_speed;
        } else {
            self.x = nx;
        }

        // Vertical sine-wave oscillation (no gravity, no collision).
        self.oscillation_tick += 1;
        let half = OSCILLATION_HALF_PERIOD;
        let dy = if self.oscillation_tick <= half {
            OSCILLATION_AMPLITUDE
        } else {
            -OSCILLATION_AMPLITUDE
        };
        self.y += dy;
        if self.oscillation_tick >= half * 2 {
            self.oscillation_tick = 0;
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

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}
