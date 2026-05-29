//! Vine / tree climb background (`BASETREE`).
//!
//! Mirrors `org.jill.game.entities.back.BaseTreeBackgroundEntity` from the
//! Java reference: the player can grab and climb the cell.  The
//! [`crate::entities::objects::player::PlayerEntity`] climb state probes the
//! [`BackgroundEntity::is_climbable`] flag at the player's centre cell, so
//! reporting `true` here is the load-bearing signal that turns this cell into
//! a vine.

use openjill_core::{BackgroundEntity, MessageDispatcher, ObjectEntity, RenderCommand};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// `BASETREE` cell handler.
pub struct BaseTreeBackground {
    /// Inner standard cell handler used for tile rendering.
    inner: StdBackgroundEntity,
}

impl BaseTreeBackground {
    /// Builds a `BASETREE` cell from a DMA `map_code`.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for BaseTreeBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: vines do not react to a passive overlap; the player drives the
    /// climb transition via its own state machine.
    fn on_player_touch(
        &mut self,
        _player: &mut dyn ObjectEntity,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Vine cells are always passable so the player can walk through them.
    fn is_passthrough(&self) -> bool {
        true
    }

    /// Climbability follows the DMA `is_vine` flag on the underlying cell, like
    /// every other background.  BASETREE itself carries bit-2-clear flags in
    /// JILL.DMA (so it is not climbable); the real climbables (VNORM, POLET,
    /// VINEBR, …) are routed to [`StdBackgroundEntity`] and pick up the flag
    /// there.  Delegating here keeps both paths consistent with Java.
    fn is_climbable(&self) -> bool {
        self.inner.is_climbable()
    }

    /// Vine cells are not stair cells.
    fn is_stair(&self) -> bool {
        false
    }
}
