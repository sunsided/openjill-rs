//! Lava-cell background hazard.
//!
//! Mirrors `org.jill.game.entities.back.KillLavaBackgroundEntity` from the Java
//! reference (`background_manager_mapping.properties` maps the DMA names
//! `LAVA1`-`LAVA5` to this class).  Cells render their DMA-defined tile and,
//! when the player overlaps the cell, classify the death as
//! [`DeathKind::OtherBackground`] and dispatch [`MessageType::DieRestartLevel`]
//! so the level screen restarts the current level.
//!
//! The Java reference uses a separate `Die*Const` sub-state for lava (visually
//! identical to the generic "other background" die animation), so the Rust
//! port routes the kill through [`DeathKind::OtherBackground`] to match the
//! die-frame selection in [`crate::entities::objects::player::PlayerEntity`].

use openjill_core::{
    BackgroundEntity, DeathKind, MessageDispatcher, MessagePayload, MessageType, ObjectEntity,
    RenderCommand,
};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// Lava cell background entity.
///
/// Wraps a [`StdBackgroundEntity`] so the rendered tile, passthrough, and
/// climb flags continue to come from the DMA entry and only the `on_player_touch`
/// behaviour is specialised.
pub struct KillLavaBackground {
    /// Inner standard cell handler that owns the tileset/tile and DMA flags.
    inner: StdBackgroundEntity,
}

impl KillLavaBackground {
    /// Builds a lava cell from a DMA `map_code`, reusing
    /// [`StdBackgroundEntity::for_map_code`] for tile and flag resolution.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for KillLavaBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state; the underlying tile animation is handled by
    /// the DMA-driven tile lookup when the SHA animator advances.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// Kills the player on contact: marks the player object with
    /// [`DeathKind::OtherBackground`] so its die animation picks the matching
    /// sub-state, and dispatches [`MessageType::DieRestartLevel`] so the
    /// level screen begins the restart hold.
    fn on_player_touch(
        &mut self,
        player: &mut dyn ObjectEntity,
        dispatcher: &mut MessageDispatcher,
    ) {
        player.on_kill(1, DeathKind::OtherBackground);
        dispatcher.send(MessageType::DieRestartLevel, MessagePayload::None);
    }

    /// Inherits the DMA-derived passthrough flag from the inner handler.
    fn is_passthrough(&self) -> bool {
        self.inner.is_passthrough()
    }

    /// Lava cells are never climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// Inherits the DMA-derived stair flag from the inner handler.
    fn is_stair(&self) -> bool {
        self.inner.is_stair()
    }
}
