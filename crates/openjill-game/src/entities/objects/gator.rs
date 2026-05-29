//! Gator enemy entity (JN object type 48).
//!
//! Mirrors `org.jill.game.entities.obj.GatorManager`: horizontal floor patrol;
//! can briefly submerge (alternate tile range); reverses at walls and gaps;
//! kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 39`, `numberTileSet = 4`.
//! The Java reference composites two tiles per frame: a left part and a right
//! part drawn side-by-side to form the full-width body.
//!
//! Right-facing: left part = tile `rightTileTail` + frame (tiles 0-3),
//!               right part = tile `rightTileHead` + frame (tiles 4-7).
//! Left-facing:  left part = tile `leftTileHead` + frame (tiles 12-15),
//!               right part = tile `leftTileTail` + frame (tiles 8-11).
//!
//! SHA header[39] confirms: 16 tiles, each 32×8 px.
//! Full composite width = 64 px, height = 8 px.

use openjill_core::layout::ZAPHOLD_AFTER_TOUCH;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::{blocked_ahead, floor_under_next, slide_x, sprite_dims};
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the gator frames.
///
/// REVERSE-ENGINEERED: `GatorManager.tileSet = 39` in `object_conf.json`.
/// Tileset 39 carries 16 tiles total; gator animates 4-tile head/tail
/// pairs per direction. Future engine config file should expose this.
const TILESET_INDEX: u8 = 39;
/// `rightTileTail` — drawn at x+0 when facing right.
///
/// REVERSE-ENGINEERED: from `GatorManager` config.
const RIGHT_LEFT_TILE: u16 = 0;
/// `rightTileHead` — drawn at x+TILE_W when facing right.
///
/// REVERSE-ENGINEERED: from `GatorManager` config.
const RIGHT_RIGHT_TILE: u16 = 4;
/// `leftTileHead` — drawn at x+0 when facing left.
///
/// REVERSE-ENGINEERED: from `GatorManager` config.
const LEFT_LEFT_TILE: u16 = 12;
/// `leftTileTail` — drawn at x+TILE_W when facing left.
///
/// REVERSE-ENGINEERED: from `GatorManager` config.
const LEFT_RIGHT_TILE: u16 = 8;
/// Width of one tile in pixels (SHA header[39]: 32 px per tile).
const TILE_W: i32 = 32;
/// Number of animation frames cycled per head/tail pair.
///
/// REVERSE-ENGINEERED: `GatorManager.numberTileSet = 4` in
/// `object_conf.json`. Verified at construction by
/// [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: u16 = 4;
/// Horizontal patrol speed in pixels per tick.
///
/// REVERSE-ENGINEERED.
const X_SPEED: i32 = 3;
/// Score awarded when the gator is killed.
///
/// REVERSE-ENGINEERED.
const SCORE_VALUE: i32 = 200;

pub struct GatorEntity {
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

impl GatorEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            LEFT_LEFT_TILE + NUMBER_TILE_SET,
            "GatorEntity NUMBER_TILE_SET",
        );
        let (_, h) = sprite_dims(cache, TILESET_INDEX);
        let jn_h = i32::from(item.height());
        let y_adj = if jn_h > 0 { (h - jn_h).max(0) } else { 0 };
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()) - y_adj,
            w: TILE_W * 2,
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

impl ObjectEntity for GatorEntity {
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
            // Slide flush to the wall (Java `moveObjectRightOnFloor`).
            slide_x(
                backgrounds,
                &mut self.x,
                self.y,
                self.w,
                self.h,
                self.x_speed,
            );
        } else {
            self.x_speed = -self.x_speed;
        }

        self.counter += 1;
        if self.counter >= NUMBER_TILE_SET as i32 {
            self.counter = 0;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        self.draw_multi().into_iter().next()
    }

    fn draw_multi(&self) -> Vec<RenderCommand> {
        if self.dead {
            return vec![];
        }
        let frame = (self.counter as u16).min(NUMBER_TILE_SET - 1);
        let (left_tile, right_tile) = if self.x_speed > 0 {
            (RIGHT_LEFT_TILE + frame, RIGHT_RIGHT_TILE + frame)
        } else {
            (LEFT_LEFT_TILE + frame, LEFT_RIGHT_TILE + frame)
        };
        vec![
            RenderCommand::Blit {
                tileset: TILESET_INDEX,
                tile: left_tile,
                x: self.x,
                y: self.y,
                opaque: false,
                clip: None,
            },
            RenderCommand::Blit {
                tileset: TILESET_INDEX,
                tile: right_tile,
                x: self.x + TILE_W,
                y: self.y,
                opaque: false,
                clip: None,
            },
        ]
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
