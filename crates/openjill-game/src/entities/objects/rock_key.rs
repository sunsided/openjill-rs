//! Rock key pickup entity (JN object type 33).
//!
//! Mirrors `org.jill.game.entities.obj.RockKeyManager` from the Java
//! reference.  Despite the entity's name, the Java `object_conf.json` has
//! `RockKeyManager.inventory = "GEM"`: a rock key pickup actually places an
//! [`InventoryObject::Gem`] entry in the inventory, which the world-map
//! [`crate::entities::objects::lock_door::LockedDoorEntity`] consumes to open
//! `MAPDOOR` cells.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset index for rock-key sprites (matches `RockKeyManager.tileSet = 9`).
const TILESET: u8 = 9;

/// Base tile index within [`TILESET`] (matches `RockKeyManager.tile = 4`).
const BASE_TILE: u16 = 4;

/// Number of animation frames (matches `RockKeyManager.numberTileSet = 4`).
const FRAME_COUNT: u16 = 4;

/// Rock key pickup entity (the gem that opens map doors).
pub struct RockKeyEntity {
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
    /// `true` once the rock key has been touched by the player.
    removed: bool,
}

impl RockKeyEntity {
    /// Builds a rock key pickup from a JN object record.
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
        }
    }
}

impl ObjectEntity for RockKeyEntity {
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

    /// Dispatches the gem inventory pickup and flags the entity for removal.
    ///
    /// `RockKeyManager` in the Java reference adds the `GEM` icon (the world
    /// map's `MAPDOOR` background looks for it via `inventory_MAPDOOR=GEM`).
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::Gem)),
        );
        self.removed = true;
    }

    /// Rock keys are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the pickup's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the rock key has been touched.
    fn should_remove(&self) -> bool {
        self.removed
    }
}
