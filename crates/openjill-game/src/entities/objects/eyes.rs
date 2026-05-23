//! Eyes enemy entity (JN object type 64).
//!
//! Mirrors `org.jill.game.entities.obj.EyesManager`: tracks the player
//! horizontally at a fixed speed; stays at its spawn Y; kills player on
//! contact.
//!
//! Tileset/tile: `tileSet = 13`, `tile = 0`, `numberTileSet = 2`.
//! FIXME(epic-6): confirm tileset 13 tiles 0..=1 against JILL1.SHA dump.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

const TILESET_INDEX: u8 = 13;
const TILE_BASE: u16 = 0;
const NUMBER_TILE_SET: u16 = 2;
const SCORE_VALUE: i32 = 300;
const CHASE_SPEED: i32 = 3;

pub struct EyesEntity {
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

impl EyesEntity {
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

impl ObjectEntity for EyesEntity {
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

        let dx = self.player_x - self.x;
        if dx.abs() > CHASE_SPEED {
            self.x += dx.signum() * CHASE_SPEED;
        } else {
            self.x = self.player_x;
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

    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
    }
}
