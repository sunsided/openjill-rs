//! "Fake floor" jump-through background.
//!
//! Mirrors `org.jill.game.entities.back.FFloorBackgroundEntity` from the Java
//! reference: the cell renders as floor but the player passes through it from
//! above (i.e. when falling) and the Java reference's
//! `AbstractBaseBackgroundEntity.isPlayerThru` reports `true`.  The Rust port
//! reports the cell as passthrough so it does not block the player; the
//! direction-aware "solid from below" half of the Java behaviour requires
//! plumbing the player's vertical velocity into the cell trait and lands as a
//! follow-up once enemies and physics need the same hook (see the parity-gap
//! notes in `docs/port/06-episode-1-gameplay.md`).

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

    /// Always passable: the player drops through the cell from above.  The
    /// Java reference also blocks upward motion through the cell; that
    /// direction-dependent half of the behaviour is a follow-up (see the
    /// module-level doc comment).
    fn is_passthrough(&self) -> bool {
        true
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
