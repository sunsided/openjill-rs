//! Knife pickup entity (JN object type 2).
//!
//! Mirrors `org.jill.game.entities.obj.KniveManager` from the Java reference:
//! when the player overlaps the knife, the entity dispatches an
//! `InventoryItem(Knife, add)` message and flags itself for removal.
//!
//! Sprite: tileset 14, tile 13 (12×12 px knife icon), same index used by
//! `BonusManager` for `KNIVE` in `object_conf.json` (`"14,13"`).

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset for the knife sprite (matches `object_conf.json` `"14,13"`).
const TILESET: u8 = 14;
/// Tile index within tileset 14 for the knife icon.
const TILE: u16 = 13;

/// Knife pickup entity.
pub struct KnifeEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// `true` once the knife has been touched by the player.
    removed: bool,
}

impl KnifeEntity {
    /// Builds a knife pickup from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            removed: false,
        }
    }
}

impl ObjectEntity for KnifeEntity {
    /// Knives are inert between touches.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile: TILE,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Dispatches the knife inventory pickup and flags the entity for removal.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::add(InventoryObject::Knife)),
        );
        self.removed = true;
    }

    /// Knife pickups are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the pickup's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the knife has been touched.
    fn should_remove(&self) -> bool {
        self.removed
    }
}
