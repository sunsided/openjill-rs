//! "Fake floor" jump-through background.
//!
//! Mirrors `org.jill.game.entities.back.FFloorBackgroundEntity` from the Java
//! reference: visually a floor that the player drops through from above when
//! falling but bumps into from below when jumping up.  [`Self::is_passthrough`]
//! returns `true` so the cell does not appear in the standard "is this cell
//! solid?" probes, while [`Self::blocks_vertical`] reports `true` only for
//! upward motion (`player_yd < 0`), turning the cell into a one-way solid for
//! the player's jump path.  Floor-detection probes use `player_yd > 0` (the
//! falling convention), where `FFLOOR` correctly reports passable and the
//! player keeps falling through.

use openjill_core::{BackgroundEntity, MessageDispatcher, ObjectEntity, RenderCommand};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// `FFLOOR` cell handler.
pub struct FFloorBackground {
    /// Inner standard cell handler used for tile rendering.
    inner: StdBackgroundEntity,
}

impl FFloorBackground {
    /// Builds an `FFLOOR` cell from a DMA `map_code`.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for FFloorBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: fall-through floors do not react to player overlap.
    fn on_player_touch(
        &mut self,
        _player: &mut dyn ObjectEntity,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Always passable for the generic solidity probe so callers that do not
    /// pass a direction (horizontal collision, stair checks) keep treating the
    /// cell as open air.  Direction-aware vertical motion goes through
    /// [`Self::blocks_vertical`] instead.
    fn is_passthrough(&self) -> bool {
        true
    }

    /// Blocks the player only while rising (`player_yd < 0`); a falling or
    /// stationary player drops through unimpeded.  Mirrors the original
    /// "jump into floor, fall through floor" semantics.
    fn blocks_vertical(&self, player_yd: i32) -> bool {
        player_yd < 0
    }

    /// `FFLOOR` cells are not climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// `FFLOOR` cells are never stair cells.
    fn is_stair(&self) -> bool {
        false
    }
}
