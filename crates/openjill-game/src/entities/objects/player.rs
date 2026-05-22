//! Player object entity (JN object type 0).
//!
//! Issue 57 introduces the type so the factory can return a distinct concrete
//! value for type 0 and so the per-tick loop can flag the player via
//! [`ObjectEntity::is_player`].  The full movement, state machine, collision,
//! and die animation land in child issue 2 of epic 6
//! (`docs/port/06-episode-1-gameplay.md`); this revision intentionally keeps
//! `update`, `draw`, `on_touch`, and `on_kill` as no-ops.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Player object entity.
///
/// Holds the spawn position and the bounding-box dimensions read from the JN
/// object record so collision callers see realistic values even before the
/// state machine lands.  The default bounding box falls back to one
/// 16x16 block when the JN record carries zero dimensions, matching how the
/// Java reference seeds `width`/`height` from the player const tables.
pub struct PlayerEntity {
    /// Current world X position in pixels.
    x: i32,
    /// Current world Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl PlayerEntity {
    /// Builds a player entity from a JN object record.
    ///
    /// `cache` is accepted to align with the factory signature and will be
    /// consulted in child issue 2 to seed the SHA tileset reference.
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

impl ObjectEntity for PlayerEntity {
    /// Placeholder: full state-machine update lands in child issue 2.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Placeholder: no sprite output until the player sprite selection lands
    /// in child issue 2.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Placeholder: pickup and damage routing lands in later child issues.
    fn on_touch(&mut self, _dispatcher: &mut MessageDispatcher) {}

    /// Placeholder: die handling lands in child issue 2.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the player's bounding box derived from the JN object record.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Always active so the player keeps ticking outside the viewport border.
    fn always_active(&self) -> bool {
        true
    }

    /// Returns `true`: this entity represents the controllable player.
    fn is_player(&self) -> bool {
        true
    }
}
