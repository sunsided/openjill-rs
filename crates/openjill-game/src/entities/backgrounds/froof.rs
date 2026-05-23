//! "Fake roof" jump-through background.
//!
//! Mirrors `org.jill.game.entities.back.FroofBackgroundEntity` from the Java
//! reference: visually a ceiling that the player rises through from below but
//! stands on when falling from above.  [`Self::is_passthrough`] returns `true`
//! so the generic solidity probes keep treating the cell as open air;
//! [`Self::blocks_vertical`] then turns the cell into a one-way solid for
//! falling motion only (`player_yd > 0`), so a falling player lands on top of
//! the cell instead of sinking through it.

use openjill_core::{BackgroundEntity, MessageDispatcher, ObjectEntity, RenderCommand};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// `FROOF` cell handler.
pub struct FroofBackground {
    /// Inner standard cell handler used for tile rendering.
    inner: StdBackgroundEntity,
}

impl FroofBackground {
    /// Builds an `FROOF` cell from a DMA `map_code`.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for FroofBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: pass-through ceilings do not react to player overlap.
    fn on_player_touch(
        &mut self,
        _player: &mut dyn ObjectEntity,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Always passable for the generic solidity probe so horizontal collision
    /// and stair checks keep treating the cell as open air.  Direction-aware
    /// vertical motion goes through [`Self::blocks_vertical`] instead.
    fn is_passthrough(&self) -> bool {
        true
    }

    /// Blocks the player only while falling (`player_yd > 0`).  A rising or
    /// stationary player rises through unimpeded; a falling player lands on
    /// the cell's top edge.  Mirrors the original "land on roof, jump through
    /// roof" semantics.
    fn blocks_vertical(&self, player_yd: i32) -> bool {
        player_yd > 0
    }

    /// `FROOF` cells are not climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// `FROOF` cells are never stair cells.
    fn is_stair(&self) -> bool {
        false
    }
}
