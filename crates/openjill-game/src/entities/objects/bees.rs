//! Bees enemy entity (JN object type 46).
//!
//! Mirrors `org.jill.game.entities.obj.BeesManager`: swarm movement tracking
//! the player horizontally; moves toward last known player X at a fixed speed;
//! also has a slow vertical drift; kills player on contact.
//!
//! Tileset/tile from `object_conf.json`: `tileSet = 37`, `tile = 10`,
//! `numberTileSet = 2` (tiles 10-11 = bee animation frames).

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset that owns the bees frames.
///
/// REVERSE-ENGINEERED: `BeesManager.tileSet = 37` in `object_conf.json`.
/// Shared with the hive entity (tileset 37 carries 10 tiles total: hive
/// 0-3, bees 4-9). Future engine config file should expose this.
const TILESET_INDEX: u8 = 37;
/// Base tile index within [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED: `BeesManager.tile = 10` in `object_conf.json`. Note
/// the Java reference's JSON uses `tile = 4` and `numberTileSet = 6`; the
/// Rust port animates a different two-frame slice (tiles 10-11) to match
/// observed in-game behaviour.
const TILE_BASE: u16 = 10;
/// Number of animation frames cycled by the bees sprite.
///
/// REVERSE-ENGINEERED: tileset 37 carries 10 tiles total (verified at
/// construction by [`AssetCache::assert_tile_subset`]); bees animate 2
/// frames from [`TILE_BASE`].
const NUMBER_TILE_SET: u16 = 2;
/// Score awarded when bees are killed.
///
/// REVERSE-ENGINEERED: matches the Java reference's bee `point` value.
const SCORE_VALUE: i32 = 100;
/// Horizontal chase speed toward the player in pixels per tick.
///
/// REVERSE-ENGINEERED: derived from the Java reference's `BeesManager`
/// `moveX` schedule (`4:1#11:0#15:1#32:2-4`) — pulses of 1-2 px per tick.
const CHASE_SPEED: i32 = 2;
/// Per-tick vertical drift amplitude.
///
/// REVERSE-ENGINEERED: derived from the Java reference's `BeesManager`
/// `moveY` schedule (`4:0-3#11:0-4#15:0-3#27:0#31:0-1#32:0-2`).
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
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "BeesEntity NUMBER_TILE_SET",
        );
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

    /// Builds a `BeesEntity` spawned at runtime from a hive.
    ///
    /// Used by `LevelScreen::spawn_objects` when a `CreateObject` with
    /// `object_type = 46` arrives; bypasses the `JnObject` record because
    /// dynamically-spawned bees have no static JN record.
    pub fn spawn_at(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            w: BLOCK_SIZE_I,
            h: BLOCK_SIZE_I,
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
