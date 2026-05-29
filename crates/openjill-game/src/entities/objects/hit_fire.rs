//! Hit-fire effect entity (JN object type 37).
//!
//! Rust translation of `HitFireManager` from the Java reference.
//!
//! A short-lived visual sprite spawned at the point of impact when a bullet
//! or weapon strikes a surface or enemy.  The entity plays a fixed-length
//! animation and then removes itself; it carries no collision logic.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Number of ticks the hit-fire effect lives before auto-removal.
///
/// Mirrors `HitFireManager.STATE_COUNT_MAX` from the Java reference.
const HIT_FIRE_LIFETIME: i32 = 8;

/// Hit-fire visual effect entity.
pub struct HitFireEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Remaining lifetime in ticks; entity removes itself when this reaches zero.
    counter: i32,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.
    origin: JnObject,
}

impl HitFireEntity {
    /// Builds a hit-fire entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: HIT_FIRE_LIFETIME,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for HitFireEntity {
    /// Decrements the lifetime counter each tick.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        if self.counter > 0 {
            self.counter -= 1;
        }
    }

    /// Sprite rendering deferred pending SHA tileset verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Hit-fire effects do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Hit-fire effects are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the effect's bounding box for culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the animation has finished.
    fn should_remove(&self) -> bool {
        self.counter <= 0
    }

    /// Snapshots the live hit-fire for a save game, or `None` once expired.
    ///
    /// The fixed lifetime restarts on restore (the effect is transient), so
    /// only the authored record (position) is persisted.
    fn snapshot(&self) -> Option<JnObject> {
        if self.counter <= 0 {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }
}
