//! Firebird player entity stub (JN object type 56).
//!
//! Placeholder for the second player form Jill adopts after collecting the
//! Firebird power-up.  Only one player entity is active at a time;
//! `ChangePlayerCharacter` messages switch between `PlayerEntity` (type 0)
//! and this entity.
//!
//! Full implementation is deferred to a follow-up issue.  This stub accepts
//! the type-56 byte from the JN object list so the factory does not fall
//! through to the warning-emitting [`StubObjectEntity`].

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Firebird player entity stub.
pub struct FirebirdPlayerEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl FirebirdPlayerEntity {
    /// Builds a Firebird player stub from a JN object record.
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

impl ObjectEntity for FirebirdPlayerEntity {
    /// Stub: no per-tick logic implemented yet.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Stub: no sprite implemented yet.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Stub: no touch reaction implemented yet.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Stub: not killed by weapons yet.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true`: this stub represents the Firebird player form.
    fn is_player(&self) -> bool {
        true
    }
}
