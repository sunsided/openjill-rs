//! Firebird weapon entity stub (JN object type 62).
//!
//! Placeholder for the projectile fired by the Firebird player form.  In
//! the Java reference (`FirebirdWeaponManager`) the projectile tracks the
//! player position and removes itself on collision.
//!
//! Full implementation is deferred until the Firebird player form and
//! enemy data are verified against the original episode 1 data.  This stub
//! accepts the type-62 byte from the JN object list so the factory does not
//! fall through to the warning-emitting [`StubObjectEntity`].

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Firebird weapon entity stub.
pub struct FirebirdWeaponEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick.
    xd: i32,
    /// Vertical velocity in pixels per tick.
    yd: i32,
}

impl FirebirdWeaponEntity {
    /// Builds a Firebird weapon stub from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            xd: i32::from(item.x_speed()),
            yd: i32::from(item.y_speed()),
        }
    }
}

impl ObjectEntity for FirebirdWeaponEntity {
    /// Stub: advances position; no homing or collision implemented yet.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        self.x += self.xd;
        self.y += self.yd;
    }

    /// Stub: no sprite implemented yet.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Stub: no touch reaction implemented yet.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Stub: not destroyed by weapons yet.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}
