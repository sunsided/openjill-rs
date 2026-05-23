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
    /// `true` once the bonus has been touched by the player.
    removed: bool,
}

impl BonusEntity {
    /// Builds a bonus pickup from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            item: resolve_bonus_item(item.counter()),
            removed: false,
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

    /// Sprite rendering deferred until pickup tile binding lands.
    fn draw(&self) -> Option<RenderCommand> {
        None
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
}

/// Resolves a JN `counter` index to the inventory item the bonus grants.
///
/// Mirrors the Java reference's `EnumInventoryObject` index order:
///
/// | Index | Java enum     | Rust [`InventoryObject`] |
/// |------:|---------------|--------------------------|
/// | 0     | `JILL`        | `Jill`                   |
/// | 1     | `RED_KEY`     | `Key`                    |
/// | 2     | `KNIVE`       | `Knife`                  |
/// | 3     | `GEM`         | `Gem`                    |
/// | 4     | `FROG`        | `FireFlower`             |
/// | 5     | `FIREBIRD`    | `Firebird`               |
/// | 8     | `BLADE`       | `Blade`                  |
///
/// Indices that the Java enum lists but the Rust port has no first-class
/// variant for (`BAG_OF_COIN`, `FISH`, `HIGH_JUMP`, `INVINCIBILITY`) fall
/// through to [`InventoryObject::Gem`] as the safest visible default; those
/// variants land alongside the player-form-switch work in issue 63.
fn resolve_bonus_item(counter: i16) -> InventoryObject {
    match counter {
        0 => InventoryObject::Jill,
        1 => InventoryObject::Key,
        2 => InventoryObject::Knife,
        3 => InventoryObject::Gem,
        4 => InventoryObject::FireFlower,
        5 => InventoryObject::Firebird,
        8 => InventoryObject::Blade,
        _ => InventoryObject::Gem,
    }
}
