//! Spike-cell background hazard.
//!
//! Mirrors `org.jill.game.entities.back.SpikeBackgroundEntity` from the Java
//! reference (`background_manager_mapping.properties` binds the `SPIKE` DMA
//! name to this class).  Lethal on overlap; renders the DMA-supplied tile.

use openjill_core::{
    BackgroundEntity, DeathKind, MessageDispatcher, MessagePayload, MessageType, ObjectEntity,
    RenderCommand,
};

use crate::asset_cache::AssetCache;
use crate::entities::backgrounds::standard::StdBackgroundEntity;

/// Spike-cell background entity.
pub struct SpikeBackground {
    /// Inner standard cell handler used for tile rendering and DMA flags.
    inner: StdBackgroundEntity,
}

impl SpikeBackground {
    /// Builds a spike cell from a DMA `map_code`.
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        Self {
            inner: StdBackgroundEntity::for_map_code(map_code, cache),
        }
    }
}

impl BackgroundEntity for SpikeBackground {
    /// Delegates the blit to the inner standard cell handler.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        self.inner.draw(screen_x, screen_y)
    }

    /// No per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// Kills the player on contact with the [`DeathKind::OtherBackground`]
    /// classification used for non-water hazards.
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

    /// Spike cells are never climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// Inherits the DMA-derived stair flag from the inner handler.
    fn is_stair(&self) -> bool {
        self.inner.is_stair()
    }
}
