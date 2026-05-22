//! Apple pickup entity (JN object type 1).
//!
//! Issue 57 introduces the type so the factory can return a distinct concrete
//! value for type 1.  Inventory dispatch on touch lands in child issue 4 of
//! epic 6; this revision intentionally keeps `on_touch` as a no-op.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Apple pickup entity placeholder.
pub struct AppleEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl AppleEntity {
    /// Builds an apple from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
        }
    }
}

impl ObjectEntity for AppleEntity {
    /// Placeholder: pickup logic lands in child issue 4.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Placeholder: sprite rendering lands in child issue 4.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Placeholder: inventory dispatch lands in child issue 4.
    fn on_touch(&mut self, _dispatcher: &mut MessageDispatcher) {}

    /// Apples are not damaged by weapons; this remains a no-op.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the apple's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}
