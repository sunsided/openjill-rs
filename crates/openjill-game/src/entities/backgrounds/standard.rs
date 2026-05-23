//! Standard background cell handler.
//!
//! Default `BackgroundEntity` returned by [`super::make_background_entity`]
//! for every DMA name.  Solid or passthrough behaviour is derived from the
//! DMA flag bits (`is_player_thru`, `is_stair`, `is_vine`); rendering blits
//! the cell's tileset/tile at the supplied framebuffer position with the
//! game-area clip rectangle so partial-overlap edge tiles do not bleed past
//! the game-area border.

use openjill_core::{BackgroundEntity, MessageDispatcher, ObjectEntity, RenderCommand};
use openjill_data::dma::DmaEntry;

use crate::asset_cache::AssetCache;
use crate::status_bar::GAME_AREA_CLIP;

/// Standard background cell handler.
///
/// `None` `tile` means the cell carries no DMA entry and is treated as
/// transparent air: it draws nothing and lets the player pass through.
pub struct StdBackgroundEntity {
    /// SHA tileset index from the DMA entry, when one exists.
    tileset: Option<u8>,
    /// Tile index inside the tileset from the DMA entry, when one exists.
    tile: Option<u16>,
    /// `true` when the player passes through the cell vertically.
    passthrough: bool,
    /// `true` when the cell behaves as a stair or slope.
    stair: bool,
    /// DMA map code for this cell, used by entities that need to identify the
    /// tile type at their grid position (e.g. `LockedDoorEntity`).
    map_code: Option<u16>,
}

impl StdBackgroundEntity {
    /// Builds a cell handler for the supplied DMA `map_code`.
    ///
    /// Looks the map code up against `cache.dma`; when no entry exists the
    /// cell is treated as transparent air (no draw output, passable).
    pub fn for_map_code(map_code: u16, cache: &AssetCache) -> Self {
        match cache.dma.get_by_map_code(map_code) {
            Some(entry) => Self::from_dma_entry(entry),
            None => Self::transparent(),
        }
    }

    /// Builds a cell handler from a resolved DMA entry.
    pub fn from_dma_entry(entry: &DmaEntry) -> Self {
        Self {
            tileset: Some(entry.tileset()),
            tile: Some(u16::from(entry.tile())),
            passthrough: entry.is_player_thru(),
            stair: entry.is_stair(),
            map_code: Some(entry.map_code()),
        }
    }

    /// Builds a transparent cell handler that draws nothing and is fully
    /// passable.
    pub fn transparent() -> Self {
        Self {
            tileset: None,
            tile: None,
            passthrough: true,
            stair: false,
            map_code: None,
        }
    }
}

impl BackgroundEntity for StdBackgroundEntity {
    /// Emits a `RenderCommand::Blit` for the cell's tile at the supplied
    /// framebuffer coordinates, or `None` when the cell is transparent.
    ///
    /// The emitted blit carries the game-area clip rectangle so partial
    /// overlaps at the game-area edge clip pixel-perfectly instead of
    /// bleeding into the status-bar frame.
    fn draw(&self, screen_x: i32, screen_y: i32) -> Option<RenderCommand> {
        let (tileset, tile) = match (self.tileset, self.tile) {
            (Some(ts), Some(t)) => (ts, t),
            _ => return None,
        };
        Some(RenderCommand::Blit {
            tileset,
            tile,
            x: screen_x,
            y: screen_y,
            opaque: false,
            clip: Some(GAME_AREA_CLIP),
        })
    }

    /// No-op: standard cells carry no per-tick dynamic state.
    fn update(&mut self, _cell_x: i32, _cell_y: i32, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: standard cells do not react to a player overlap.
    fn on_player_touch(
        &mut self,
        _player: &mut dyn ObjectEntity,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Returns the player-through flag derived from the DMA entry.
    fn is_passthrough(&self) -> bool {
        self.passthrough
    }

    /// Standard cells are never climbable; only [`super::base_tree::BaseTreeBackground`]
    /// (the BASETREE entity) returns `true`.  The DMA `is_vine()` opt-out flag
    /// defaults to vine for nearly every tile in JILL.DMA, so reading it here
    /// would incorrectly mark shade and decoration tiles as climbable.
    fn is_climbable(&self) -> bool {
        false
    }

    /// Returns the stair flag derived from the DMA entry.
    fn is_stair(&self) -> bool {
        self.stair
    }

    /// Returns the DMA map code for this cell, when one was set at
    /// construction via [`Self::from_dma_entry`] or [`Self::for_map_code`].
    fn dma_map_code(&self) -> Option<u16> {
        self.map_code
    }
}
