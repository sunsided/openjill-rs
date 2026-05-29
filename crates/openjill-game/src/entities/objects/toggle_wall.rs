//! Toggle-wall entity (JN object type 26).
//!
//! Rust translation of `ToggleWallManager` from the Java reference
//! (`open-jill-object-background`).
//!
//! A toggle wall starts in its "active" (solid) state and flips to
//! "inactive" (passthrough) when it receives a [`MessageType::Trigger`]
//! message whose link identifier matches its own [`link_id`].  A second
//! trigger toggles it back.
//!
//! Trigger routing is handled by the level loop: after each tick the loop
//! drains any `Trigger` messages dispatched during that tick and calls
//! [`ObjectEntity::receive_trigger`] on every object.  This entity overrides
//! that hook and toggles when the link identifier matches.
//!
//! # Collision note
//!
//! In the Java reference, `ToggleWallManager` is part of the
//! `open-jill-object-background` package and acts as both an object entity
//! and a background tile.  The Rust port models it as a pure object entity,
//! so the `active` flag is tracked correctly but currently has no effect on
//! player movement collision.  Player movement checks the `BackgroundGrid`
//! only; toggle wall bboxes are not yet consulted.  Full collision integration
//! requires extending `PlayerEntity::update` (or the movement sub-routines) to
//! also test blocking-object bboxes exposed via a future `is_solid_blocker()`
//! trait hook.  Until that follow-up lands, the toggle state is observable
//! through [`ToggleWallEntity::is_active`] but does not prevent the player
//! from passing through the wall.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Toggle-wall entity.
pub struct ToggleWallEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Link identifier used to match incoming `Trigger` messages.
    ///
    /// Copied from the JN `counter` field.
    link_id: i32,
    /// `true` when the wall is in its solid (blocking) state.
    active: bool,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.  The
    /// authored `counter` (trigger link id) is preserved untouched.
    origin: JnObject,
}

impl ToggleWallEntity {
    /// Builds a toggle-wall entity from a JN object record.
    ///
    /// Walls start active (solid) by default.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            link_id: i32::from(item.counter()),
            active: true,
            origin: item.clone(),
        }
    }

    /// Returns `true` when the wall is currently in its solid state.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl ObjectEntity for ToggleWallEntity {
    /// Toggle walls carry no per-tick update logic.
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

    /// Toggle walls do not react to player touch directly.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// Toggle walls are not destroyed by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the wall's bounding box.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Snapshots the wall for a save game (always persisted).
    ///
    /// The solid/passthrough toggle re-derives from the linked switch on
    /// restore, so only the authored record (position + link id) is persisted.
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }

    /// Toggles the solid/passthrough state when `link_id` matches.
    fn receive_trigger(&mut self, link_id: i32) {
        if link_id == self.link_id {
            self.active = !self.active;
        }
    }
}
