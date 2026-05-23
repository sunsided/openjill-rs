//! Bees enemy entity (JN object type 46).
//!
//! Mirrors `org.jill.game.entities.obj.BeesManager`: swarm movement tracking
//! the player horizontally; moves toward last known player X at a fixed speed;
//! also has a slow vertical drift; kills player on contact.
//!
//! Tileset/tile: `tileSet = 8`, `tile = 10`, `numberTileSet = 2`.
//! FIXME(epic-6): confirm tileset 8 tiles 10..=11 against JILL1.SHA dump.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

const TILESET_INDEX: u8 = 8;
const TILE_BASE: u16 = 10;
const NUMBER_TILE_SET: u16 = 2;
const SCORE_VALUE: i32 = 100;
const CHASE_SPEED: i32 = 2;
const VERTICAL_DRIFT: i32 = 1;

pub struct BeesEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    player_x: i32,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
}

impl BeesEntity {
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let x = i32::from(item.x());
        Self {
            x,
            y: i32::from(item.y()),
            w,
            h,
            player_x: x,
            counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            pending_kill: None,
        }
    }
}

impl ObjectEntity for BeesEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
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

        // Chase player horizontally.
        let dx = self.player_x - self.x;
        if dx.abs() > CHASE_SPEED {
            self.x += dx.signum() * CHASE_SPEED;
        } else {
            self.x = self.player_x;
        }

        // Slow vertical drift upward.
        self.y -= VERTICAL_DRIFT;

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

    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
    }
}
