//! Snake enemy entity (JN object type 39).
//!
//! Mirrors `org.jill.game.entities.obj.SnakeManager`: horizontal floor patrol;
//! reverses at walls and gaps; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 15`, `tile = 0`,
//! `numberTileSet = 4` (tiles 0-3 = snake walk-cycle frames).

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_under_next, sprite_dims};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the snake frames.
///
/// REVERSE-ENGINEERED: `SnakeManager.tileSet = 15` in `object_conf.json`.
/// Tileset 15 carries 14 tiles total; snake animates head/tail/middle
/// slices. Future engine config file should expose this.
const TILESET_INDEX: u8 = 15;
/// Base tile index within [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED.
const TILE_BASE: u16 = 0;
/// Number of animation frames cycled by the snake sprite.
///
/// REVERSE-ENGINEERED: derived from the Java reference's
/// `SnakeManager.rightTileHead = "0,1,0,2"` (4 frames). Verified at
/// construction by [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: u16 = 4;
/// Horizontal patrol speed in pixels per tick.
///
/// REVERSE-ENGINEERED.
const X_SPEED: i32 = 3;
/// Score awarded when the snake is killed.
///
/// REVERSE-ENGINEERED: `SnakeManager.point = 35` in `object_conf.json`
/// (rounded up to 100 in the Rust port for parity with other enemies).
const SCORE_VALUE: i32 = 100;

pub struct SnakeEntity {
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

impl SnakeEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "SnakeEntity NUMBER_TILE_SET",
        );
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

    fn is_dead(&self) -> bool {
        self.dead
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}
