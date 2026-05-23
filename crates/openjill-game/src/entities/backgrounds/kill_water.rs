//! Water-cell background hazard.
//!
//! Mirrors `org.jill.game.entities.back.KillWaterBackgroundEntity` from the
//! Java reference, which `background_manager_mapping.properties` maps onto the
//! large family of `WATERTL`/`WATERTR`/`WATERRD`/`WATERLD`/`WATERML`/`WATERMR`
//! cells (plus their `*2`-`*4` animation siblings).  Cells render their
//! DMA-defined tile and, when the player overlaps the cell, arm the player
//! die state with [`DeathKind::Water`].  The player's `Die` sub-state then
//! dispatches `DieRestartLevel` once the drowning animation has finished, so
//! the level transition overlay does not start on the touch frame.

use openjill_core::{BackgroundEntity, DeathKind, MessageDispatcher, ObjectEntity, RenderCommand};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// Water cell background entity.
pub struct KillWaterBackground {
    /// Inner standard cell handler used for tile rendering and DMA flags.
    inner: StdBackgroundEntity,
}

impl KillWaterBackground {
    /// Builds a water cell from a DMA `map_code`.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for KillWaterBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// Drowns the player on contact: classifies the death as
    /// [`DeathKind::Water`].  The player's `Die` sub-state dispatches
    /// `DieRestartLevel` once the drowning animation has finished, so the
    /// level transition overlay does not start on the touch frame.
    fn on_player_touch(
        &mut self,
        player: &mut dyn ObjectEntity,
        _dispatcher: &mut MessageDispatcher,
    ) {
        player.on_kill(1, DeathKind::Water);
    }

    /// Inherits the DMA-derived passthrough flag from the inner handler.
    fn is_passthrough(&self) -> bool {
        self.inner.is_passthrough()
    }

    /// Water cells are never climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// Inherits the DMA-derived stair flag from the inner handler.
    fn is_stair(&self) -> bool {
        self.inner.is_stair()
    }
}
