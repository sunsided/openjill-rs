//! Spark visual effect entity (JN object type 65).
//!
//! Rust translation of `SparkManager` from the Java reference.
//!
//! Short-lived spark sprite emitted on certain impacts or environmental
//! triggers.  Carries no collision logic; sprite rendering is deferred
//! until SHA tileset indices are verified against the original data.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Number of ticks a spark lives before auto-removal.
///
/// Mirrors `SparkManager.STATE_COUNT_MAX` from the Java reference.
const SPARK_LIFETIME: i32 = 6;

/// Spark visual effect entity.
pub struct SparkEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Remaining lifetime in ticks.
    counter: i32,
}

impl SparkEntity {
    /// Builds a spark entity from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter: SPARK_LIFETIME,
        }
    }
}

impl ObjectEntity for SparkEntity {
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

    /// Sparks do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Sparks are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the bounding box for culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the spark's lifetime has expired.
    fn should_remove(&self) -> bool {
        self.counter <= 0
    }
}
