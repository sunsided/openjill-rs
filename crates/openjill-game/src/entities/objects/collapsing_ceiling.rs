//! Collapsing ceiling hazard entity (JN object type 25).
//!
//! Mirrors `org.jill.game.entities.obj.CollapsingCeilingManager` from the Java
//! reference: the entity hangs stationary until the player walks under it,
//! then drops at a fixed rate.  Lethal on contact.
//!
//! The Rust port currently models the lethal-touch contract that issue 61
//! requires; the trigger (walks-under detection) and the falling motion land
//! together with the rest of the hazard-movement pass once the SHA tile
//! identity for the falling block is verified against `JILL1.SHA`.  See the
//! SHA-verification note in `docs/port/06-episode-1-gameplay.md`.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Collapsing ceiling entity.
pub struct CollapsingCeilingEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Pending player-kill classification armed in [`Self::on_touch`] and
    /// drained by the level loop via [`ObjectEntity::take_player_kill`].
    pending_kill: Option<DeathKind>,
}

impl CollapsingCeilingEntity {
    /// Builds a collapsing ceiling from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            pending_kill: None,
        }
    }
}

impl ObjectEntity for CollapsingCeilingEntity {
    /// No-op: the fall trigger and downward motion are follow-up work; see
    /// the module-level doc comment.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Sprite rendering is deferred until the SHA tileset identity for the
    /// falling-block frame is verified.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Arms a [`DeathKind::OtherBackground`] kill that the level loop drains
    /// via [`Self::take_player_kill`] and applies to the player.  No direct
    /// `DieRestartLevel` dispatch: the player's `Die` sub-state fires it once
    /// the die animation finishes.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        self.pending_kill = Some(DeathKind::OtherBackground);
    }

    /// Collapsing ceilings are indestructible; weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the entity's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns the pending kill classification (and clears it) so the level
    /// loop can apply it to the player after the touch dispatch pass.
    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}
