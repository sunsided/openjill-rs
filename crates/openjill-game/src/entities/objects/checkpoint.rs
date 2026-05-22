//! Checkpoint object entity (JN object type 12).
//!
//! Issue 57 introduces the type so the factory can return a distinct concrete
//! value for type 12 and so the per-tick loop can identify the checkpoint via
//! [`ObjectEntity::is_checkpoint`].  Level-transition message dispatch lands
//! in child issue 4 of epic 6; this revision intentionally keeps `on_touch`
//! as a no-op.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Checkpoint object entity placeholder.
///
/// Carries the JN `counter` value alongside position so child issue 4 can use
/// it as the destination level number when transitioning.
pub struct CheckPointEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Destination level number copied from the JN `counter` field.
    counter: i16,
}

impl CheckPointEntity {
    /// Builds a checkpoint entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: item.counter(),
        }
    }

    /// Returns the JN `counter` value, used as the destination level number
    /// when this checkpoint fires a transition.
    pub fn counter(&self) -> i16 {
        self.counter
    }
}

impl ObjectEntity for CheckPointEntity {
    /// Placeholder: per-tick checkpoint behaviour lands in child issue 4.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Checkpoints render no sprite; the surrounding world tiles already
    /// convey the level entry point.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Placeholder: `CheckpointChangeLevel` dispatch lands in child issue 4.
    fn on_touch(&mut self, _dispatcher: &mut MessageDispatcher) {}

    /// Checkpoints are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the checkpoint's bounding box for player overlap tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true`: this entity is the level-transition marker.
    fn is_checkpoint(&self) -> bool {
        true
    }
}
