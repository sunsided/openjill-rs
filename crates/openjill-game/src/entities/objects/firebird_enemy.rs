//! Firebird enemy entity (JN object type 30).
//!
//! Mirrors `org.jill.game.entities.obj.FirebirdManager`: flying enemy that
//! moves purely horizontally; reverses when hitting a wall or after cycling
//! through its full animation sequence; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 11`, `tile = 0`,
//! `baseTileNumber = 4`, `turnTileNumber = 1`.
//! Right-facing: tiles 0-3.  Left-facing: tiles 4-7.
//! SHA header[11] confirms: 12 tiles, 16×16 px each.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;
use crate::entities::objects::enemy_shared::sprite_dims;

const TILESET_INDEX: u8 = 11;
/// First tile for right-facing frames (tiles 0-3).
const TILE_BASE_RIGHT: u16 = 0;
/// First tile for left-facing frames (tiles 4-7, = baseTileNumber offset).
const TILE_BASE_LEFT: u16 = 4;
const NUMBER_TILE_SET: u16 = 4;
const SCORE_VALUE: i32 = 300;
const DEFAULT_X_SPEED: i32 = 4;

pub struct FirebirdEnemyEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    x_speed: i32,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
}

impl FirebirdEnemyEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let xd = i32::from(item.x_speed());
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed: if xd != 0 { xd } else { DEFAULT_X_SPEED },
            counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
        }
    }

    /// `true` when the bounding box overlaps a solid cell or is outside the map.
    fn collides_solid(&self, backgrounds: &BackgroundGrid, nx: i32, ny: i32) -> bool {
        let map_w = (backgrounds.width as i32) * BLOCK_SIZE_I;
        let map_h = (backgrounds.height as i32) * BLOCK_SIZE_I;
        if nx < 0 || ny < 0 || nx + self.w > map_w || ny + self.h > map_h {
            return true;
        }
        let cx_l = nx.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cx_r = (nx + self.w - 1)
            .div_euclid(BLOCK_SIZE_I)
            .max(0)
            .min((backgrounds.width as i32) - 1) as usize;
        let cy_t = ny.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cy_b = (ny + self.h - 1)
            .div_euclid(BLOCK_SIZE_I)
            .max(0)
            .min((backgrounds.height as i32) - 1) as usize;
        for cy in cy_t..=cy_b {
            for cx in cx_l..=cx_r {
                if let Some(cell) = backgrounds.get(cx, cy)
                    && !cell.is_passthrough()
                {
                    return true;
                }
            }
        }
        false
    }
}

impl ObjectEntity for FirebirdEnemyEntity {
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

        let nx = self.x + self.x_speed;

        if self.collides_solid(backgrounds, nx, self.y) {
            self.x_speed = -self.x_speed;
        } else {
            self.x = nx;
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
        let base = if self.x_speed < 0 {
            TILE_BASE_LEFT
        } else {
            TILE_BASE_RIGHT
        };
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: base + frame,
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
