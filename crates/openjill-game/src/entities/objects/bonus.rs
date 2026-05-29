//! Bonus pickup entity (JN object type 28).
//!
//! Mirrors `org.jill.game.entities.obj.BonusManager` from the Java reference:
//! the JN `counter` field is an index into `EnumInventoryObject`, and the
//! bonus dispatches an `InventoryItem(<mapped>, add)` message on touch and
//! flags itself for removal.
//!
//! The Java port also supports "don't remove" categories (Jill, Frog,
//! Firebird, Fish) that transform the player instead of granting an item;
//! that flow lands with the player-form-switch work in issue 63.  For now
//! every bonus simply grants an inventory entry and removes itself.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, InventoryItemPayload, InventoryObject,
    MessageDispatcher, MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset index for all bonus sprites (matches `BonusManager` config `"14,*"`).
const TILESET: u8 = 14;

/// Bonus pickup entity.
pub struct BonusEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Inventory item granted on touch, resolved from the JN `counter` index.
    item: InventoryObject,
    /// SHA tile index for this bonus sprite, resolved from `BonusManager` config.
    tile: u16,
    /// `true` once the bonus has been touched by the player.
    removed: bool,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.  Its
    /// `counter` (the granted-item index) is preserved untouched.
    origin: JnObject,
}

impl BonusEntity {
    /// Builds a bonus pickup from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let resolved = resolve_bonus_item(item.counter());
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            tile: bonus_tile(resolved),
            item: resolved,
            removed: false,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for BonusEntity {
    /// Inert between touches.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Returns a `Blit` for this bonus sprite.
    ///
    /// Single frame - `BonusManager.msgDraw()` returns one static image
    /// with no animation cycle.
    fn draw(&self) -> Option<RenderCommand> {
        Some(RenderCommand::Blit {
            tileset: TILESET,
            tile: self.tile,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Dispatches the resolved inventory pickup and flags the entity for
    /// removal.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        dispatcher.send(
            MessageType::InventoryItem,
            MessagePayload::InventoryItem(InventoryItemPayload::add(self.item)),
        );
        self.removed = true;
    }

    /// Bonus pickups are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the pickup's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the bonus has been touched.
    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Snapshots the bonus for a save game, or `None` once collected.
    ///
    /// All authored fields, including the `counter` that selects the granted
    /// item, are preserved from the cloned origin.
    fn snapshot(&self) -> Option<JnObject> {
        if self.removed {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }
}

/// Resolves a JN `counter` index to the inventory item the bonus grants.
///
/// The `counter` is an `EnumInventoryObject` index; resolution delegates to
/// [`InventoryObject::from_index`], which covers all 11 indices.  A negative or
/// out-of-range counter falls back to [`InventoryObject::Gem`] as the safest
/// visible default.
fn resolve_bonus_item(counter: i16) -> InventoryObject {
    u16::try_from(counter)
        .ok()
        .and_then(InventoryObject::from_index)
        .unwrap_or(InventoryObject::Gem)
}

/// Maps an inventory item to the tile index within tileset 14 used by
/// `BonusManager` (values from `object_conf.json`'s `BonusManager` block).
fn bonus_tile(item: InventoryObject) -> u16 {
    match item {
        InventoryObject::Jill => 38,
        InventoryObject::RedKey => 12,
        InventoryObject::Knife => 13,
        InventoryObject::Gem => 11,
        InventoryObject::Frog => 14,
        InventoryObject::Firebird => 15,
        InventoryObject::BagOfCoin => 18,
        InventoryObject::Fish => 20,
        InventoryObject::Blade => 35,
        InventoryObject::HighJump => 36,
        InventoryObject::Invincibility => 37,
    }
}
