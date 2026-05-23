//! "Fake roof" jump-through background.
//!
//! Mirrors `org.jill.game.entities.back.FroofBackgroundEntity` from the Java
//! reference: visually a ceiling that the player can jump up through.  The
//! Rust port reports the cell as passthrough so it never blocks the player;
//! the Java reference's direction-aware "solid from above" half lands as a
//! follow-up once the physics hook is in place (see the parity-gap notes in
//! `docs/port/06-episode-1-gameplay.md`).

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

    /// Always passable: the player rises through the cell from below.
    fn is_passthrough(&self) -> bool {
        true
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
