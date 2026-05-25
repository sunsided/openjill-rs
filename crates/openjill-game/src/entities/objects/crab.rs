//! Crab enemy entity (JN object type 47).
//!
//! Mirrors `org.jill.game.entities.obj.CrabManager`: horizontal floor patrol;
//! reverses at walls and gaps; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 38`, `tile = 0`,
//! `numberTileSet = 4`.
//! SHA header[38] confirms: crab sprites, 4 walk-cycle tiles.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_under_next, sprite_dims};
use crate::asset_cache::AssetCache;

const TILESET_INDEX: u8 = 38;
const TILE_BASE: u16 = 0;
const NUMBER_TILE_SET: u16 = 4;
const X_SPEED: i32 = 4;
const SCORE_VALUE: i32 = 200;

pub struct CrabEntity {
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

impl CrabEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let jn_h = i32::from(item.height());
        let y_adj = if jn_h > 0 { (h - jn_h).max(0) } else { 0 };
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()) - y_adj,
            w,
            h,
            x_speed: X_SPEED,
            counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
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
        if !blocked_ahead(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
            && floor_under_next(backgrounds, self.x, self.y, self.w, self.h, self.x_speed)
        {
            self.x += self.x_speed;
        } else {
            self.x_speed = -self.x_speed;
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
