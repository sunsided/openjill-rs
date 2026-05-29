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

/// SHA tileset index for rock-key sprites.
///
/// REVERSE-ENGINEERED: `RockKeyManager.tileSet = 9` in `object_conf.json`.
/// Not derivable from SHA structure; future engine config file should expose
/// this.
const TILESET: u8 = 9;

/// Base tile index within [`TILESET`].
///
/// REVERSE-ENGINEERED: `RockKeyManager.tile = 4` in `object_conf.json`.
const BASE_TILE: u16 = 4;

/// Number of animation frames cycled by the rock-key sprite.
///
/// REVERSE-ENGINEERED: `RockKeyManager.numberTileSet = 4` in
/// `object_conf.json`. Tileset 9 carries 8 tiles total (verified at
/// construction by [`AssetCache::assert_tile_subset`]); rock key animates
/// tiles 4-7.
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
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.
    origin: JnObject,
}

impl RockKeyEntity {
    /// Builds a rock key pickup from a JN object record.
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET,
            BASE_TILE + FRAME_COUNT,
            "RockKeyEntity FRAME_COUNT",
        );
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

    /// Snapshots the rock key for a save game, or `None` once collected.
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
