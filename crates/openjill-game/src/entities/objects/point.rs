//! Floating score popup entity (JN object type 27).
//!
//! Mirrors `org.jill.game.entities.obj.PointManager` from the Java reference:
//! the entity is spawned with a counter ticking down every frame and removes
//! itself when the counter reaches zero.  No touch behaviour - this is a
//! visual-only popup that lifts away from the spawn point while showing the
//! awarded score.
//!
//! Sprite rendering (the small-number glyphs) is deferred until the
//! `TextManager` port lands; for now the entity exists so the level loop
//! does not fall through to [`crate::entities::objects::stub::StubObjectEntity`]
//! and so its lifetime is correctly bounded.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Floating score popup entity.
pub struct PointEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Horizontal velocity in pixels per tick.
    x_speed: i32,
    /// Vertical velocity in pixels per tick (negative = upward).
    y_speed: i32,
    /// Remaining lifetime in ticks; once `counter` reaches zero the entity
    /// flags itself for removal.
    counter: i32,
}

impl PointEntity {
    /// Builds a floating score popup from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            x_speed: i32::from(item.x_speed()),
            y_speed: i32::from(item.y_speed()),
            counter: i32::from(item.counter()).max(0),
        }
    }
}

impl ObjectEntity for PointEntity {
    /// Advances position, decrements the lifetime counter, and decays
    /// horizontal velocity toward zero.  Mirrors the per-tick velocity decay
    /// used by `PointManager.msgUpdate` in the Java reference.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        if self.counter <= 0 {
            return;
        }
        self.counter -= 1;
        self.x += self.x_speed;
        self.y += self.y_speed;
        if self.x_speed > 0 {
            self.x_speed -= 1;
        } else if self.x_speed < 0 {
            self.x_speed += 1;
        }
        self.y_speed -= 1;
    }

    /// Sprite rendering deferred until the small-number glyph helper lands.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Floating popups do not react to player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Floating popups are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the popup's bounding box for culling.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the popup's lifetime has expired.
    fn should_remove(&self) -> bool {
        self.counter <= 0
    }
}

#[cfg(test)]
mod tests {
    use super::PointEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        ActiveInput, BACKGROUND_GRID_HEIGHT, BACKGROUND_GRID_WIDTH, BackgroundEntity,
        BackgroundGrid, MessageDispatcher, ObjectEntity, RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;

    /// Object record byte length used by the synthetic JN buffer helpers.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Builds a one-object JN file whose record has `object_type = 27` and
    /// the supplied `counter`, returning the inner `JnObject` clone.
    fn synthetic_point_object(counter: i16) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 27;
        let counter_off = record_off + 19;
        bytes[counter_off..counter_off + 2].copy_from_slice(&counter.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Inert background entity used to build the empty `BackgroundGrid`
    /// `update` requires; the point entity never queries cells but the trait
    /// still requires a non-empty grid value.
    struct EmptyBg;
    impl BackgroundEntity for EmptyBg {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            true
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds an empty `BackgroundGrid` of the canonical 128x64 dimensions.
    fn empty_grid() -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> =
            Vec::with_capacity(BACKGROUND_GRID_HEIGHT);
        for _ in 0..BACKGROUND_GRID_HEIGHT {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(BACKGROUND_GRID_WIDTH);
            for _ in 0..BACKGROUND_GRID_WIDTH {
                row.push(Box::new(EmptyBg));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Unit under test: a [`PointEntity`] with a finite counter removes
    /// itself after the counter ticks down to zero.
    #[test]
    fn update_drops_counter_to_zero_and_flags_for_removal() {
        let cache = AssetCache::synthetic();
        let mut point = PointEntity::new(&synthetic_point_object(2), &cache);
        let input = ActiveInput::new();
        let state = RuntimeState::new();
        let grid = empty_grid();
        let mut dispatcher = MessageDispatcher::new();

        point.update(&input, &state, &grid, &mut dispatcher);
        assert!(!point.should_remove());
        point.update(&input, &state, &grid, &mut dispatcher);
        assert!(
            point.should_remove(),
            "point should expire after counter ticks to zero"
        );
    }
}
