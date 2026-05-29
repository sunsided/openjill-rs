//! Red key pickup entity (JN object type 14).
//!
//! Mirrors `org.jill.game.entities.obj.RedKeyManager` from the Java reference:
//! adds an [`InventoryObject::RedKey`] entry to the inventory and flags itself
//! for removal on touch.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset index for red-key sprites (matches `RedKeyManager.tileSet = 14`).
const TILESET: u8 = 14;

/// Base tile index within [`TILESET`] (matches `RedKeyManager.tile = 6`).
const BASE_TILE: u16 = 6;

/// Number of animation frames (matches `RedKeyManager.numberTileSet = 4`).
const FRAME_COUNT: u16 = 4;

/// Red key pickup entity.
pub struct RedKeyEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Animation frame counter; incremented each tick, cycles every
    /// `FRAME_COUNT * 2` ticks so each tile is shown for 2 ticks.
    frame: u16,
    /// `true` once the key has been touched by the player.
    removed: bool,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.
    origin: JnObject,
}

impl RedKeyEntity {
    /// Builds a red key pickup from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            frame: 0,
            removed: false,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for RedKeyEntity {
    /// Advances the animation frame counter.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Returns a `Blit` for the current animation frame.
    ///
    /// Cycles through `FRAME_COUNT` tiles (each shown for 2 ticks), matching
    /// `AbstractKeyManager.msgDraw` / `msgUpdate` from the Java reference.
    fn draw(&self) -> Option<RenderCommand> {
        let tile = BASE_TILE + (self.frame / 2) % FRAME_COUNT;
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Dispatches the key inventory pickup and flags the entity for removal.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::RedKey)),
        );
        self.removed = true;
    }

    /// Red keys are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the pickup's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the key has been touched.
    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Snapshots the key for a save game, or `None` once collected.
    ///
    /// The cosmetic `frame` animation counter has no JN field and resets on
    /// restore; all authored fields are preserved from the cloned origin.
    fn snapshot(&self) -> Option<JnObject> {
        if self.removed {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }
}
