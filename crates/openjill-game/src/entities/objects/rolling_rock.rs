//! Rolling rock hazard entity (JN object type 35).
//!
//! Mirrors `org.jill.game.entities.obj.RollingRockManager` from the Java
//! reference: the rock moves horizontally along a floor, reversing direction
//! at walls or gaps, and kills the player on contact.
//!
//! Only the lethal-touch contract is modelled here.  Horizontal motion,
//! wall / gap reverse handling, and the sprite tile selection land alongside
//! the rest of the hazard-movement pass once the SHA tileset identity for the
//! rolling-rock frames is verified against `JILL1.SHA` (see the
//! SHA-verification note in `docs/port/06-episode-1-gameplay.md`).

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Rolling rock entity.
pub struct RollingRockEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
}

impl RollingRockEntity {
    /// Builds a rolling rock from a JN object record.
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

impl ObjectEntity for RollingRockEntity {
    /// No-op: horizontal motion and wall / gap detection are follow-up work.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Sprite rendering is deferred until SHA-tile verification.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Kills the player on touch by dispatching [`MessageType::DieRestartLevel`].
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
    }

    /// Rolling rocks are indestructible; weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the entity's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }
}
