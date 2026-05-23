//! Text-tile decoration entities (JN object types 20 and 21).
//!
//! Rust translation of `TextTileManager` from the Java reference.
//!
//! Renders a single SHA tile from the level's background tileset at the
//! object's world position.  Used for decorative foreground tiles that are
//! not part of the scrolling background layer.  Carries no collision logic.
//!
//! Types 20 and 21 share the same implementation; the factory registers
//! both type bytes against this entity.  Sprite rendering is deferred
//! until SHA tileset indices are verified against the original data.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Text-tile decoration entity (types 20 and 21).
pub struct TextTileEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl TextTileEntity {
    /// Builds a text-tile entity from a JN object record.
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

impl ObjectEntity for TextTileEntity {
    /// Text tiles carry no per-tick gameplay logic.
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

    /// Text tiles do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Text tiles are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the bounding box for culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}
