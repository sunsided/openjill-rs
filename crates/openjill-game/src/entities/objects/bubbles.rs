//! Underwater bubbles decoration entity (JN object type 58).
//!
//! Rust translation of `BubblesManager` from the Java reference.
//!
//! Decorative animated bubbles that rise from the floor in underwater areas.
//! Carries no collision logic; sprite rendering is deferred until SHA
//! tileset indices are verified against the original data.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Underwater bubbles decoration entity.
pub struct BubblesEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.
    origin: JnObject,
}

impl BubblesEntity {
    /// Builds a bubbles entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for BubblesEntity {
    /// Bubbles carry no per-tick gameplay logic.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Sprite rendering deferred pending SHA tileset verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Bubbles do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Bubbles are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the bounding box for culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Snapshots the decorative bubbles for a save game (always persisted).
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }
}
