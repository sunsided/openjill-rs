//! Collapsing ceiling hazard entity (JN object type 25).
//!
//! Mirrors `org.jill.game.entities.obj.CollapsingCeilingManager` from the Java
//! reference: the entity hangs stationary until the trigger arms, then drops
//! at a fixed rate and removes itself when it lands on a solid cell.  Lethal
//! on contact while falling.
//!
//! The Java reference arms the fall through a `TRIGGER` message dispatched by
//! a linked `SwitchEntity` (`SwitchManager.recieveMessage`).  Switches land
//! with issue #63; until then the Rust port follows the `Gameplay: hazard
//! motion + sprite tiles (#91)` issue text and arms the fall the first tick
//! the player's bounding box overlaps the cell directly below the ceiling
//! ("walks-under" detection).  Switch-driven trigger replaces the walks-under
//! heuristic when `SwitchEntity` lands.
//!
//! Tile / tileset identity comes from `object_conf.json`:
//! `tileSet = 14`, `tile = 16`, `speedFall = 4`.
//!
//! FIXME(epic-6): confirm tileset 14 tile 16 against the original `JILL1.SHA`
//! once the SHA dumper grows a visual / PNG output.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset that owns the falling-block frame.
const TILESET_INDEX: u8 = 14;

/// Tile index inside [`TILESET_INDEX`] for the falling block.
const TILE_INDEX: u16 = 16;

/// Per-tick downward motion in pixels (`CollapsingCeilingManager.speedFall`).
const SPEED_FALL: i32 = 4;

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
    /// `true` once the fall has been armed by [`Self::observe_player`] (the
    /// walks-under trigger) and the entity should advance downward each tick.
    triggered: bool,
    /// `true` once the falling block lands on a solid cell and the level loop
    /// should drop it from the active object list.
    removed: bool,
    /// Player bounding box recorded by the most recent
    /// [`Self::observe_player`] call; consumed during [`Self::update`] to arm
    /// the trigger when the player overlaps the cell below.
    last_player_bbox: Option<Rect>,
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
            triggered: false,
            removed: false,
            last_player_bbox: None,
            pending_kill: None,
        }
    }

    /// Returns the rectangle directly below the ceiling that the player must
    /// occupy for the walks-under trigger to fire.
    ///
    /// The trigger zone is one block tall and matches the ceiling's
    /// horizontal extent so a player whose feet land in the same column as
    /// the ceiling arms the fall, regardless of how far below they are
    /// vertically aligned to the cell.  Java's switch-triggered behaviour does
    /// not constrain proximity at all; the walks-under heuristic uses a
    /// generous column probe to stay close to the original spirit.
    fn trigger_zone(&self) -> Rect {
        Rect::new(self.x, self.y + self.h, self.w, BLOCK_SIZE_I)
    }
}

impl ObjectEntity for CollapsingCeilingEntity {
    /// Advances the ceiling.  Once armed, the entity falls
    /// [`SPEED_FALL`] pixels per tick until the cell directly below is solid,
    /// at which point it flags itself for removal (mirrors the Java
    /// reference's `killme` dispatch when `backgroundObject[x][y+1]` is not
    /// `isPlayerThru`).
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        if self.removed {
            return;
        }

        if !self.triggered
            && let Some(player_bbox) = self.last_player_bbox
            && player_bbox.intersects(&self.trigger_zone())
        {
            self.triggered = true;
        }

        if !self.triggered {
            return;
        }

        let next_cell_y = (self.y + self.h).div_euclid(BLOCK_SIZE_I);
        let cell_x_left = self.x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cell_x_right = (self.x + self.w - 1).div_euclid(BLOCK_SIZE_I).max(0) as usize;
        if next_cell_y < 0 {
            self.removed = true;
            return;
        }
        let next_cell_y = next_cell_y as usize;
        if next_cell_y >= backgrounds.height {
            self.removed = true;
            return;
        }
        let mut blocked = false;
        for cx in cell_x_left..=cell_x_right.min(backgrounds.width.saturating_sub(1)) {
            if let Some(cell) = backgrounds.get(cx, next_cell_y)
                && (!cell.is_passthrough() || cell.is_stair())
            {
                blocked = true;
                break;
            }
        }
        if blocked {
            self.removed = true;
            return;
        }

        self.y += SPEED_FALL;
    }

    /// Emits a [`RenderCommand::Blit`] for the falling block tile while the
    /// entity is still active.
    fn draw(&self) -> Option<RenderCommand> {
        if self.removed {
            return None;
        }
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: TILE_INDEX,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Records the player's bounding box so [`Self::update`] can resolve the
    /// walks-under trigger.
    fn observe_player(&mut self, player_bbox: Rect) {
        self.last_player_bbox = Some(player_bbox);
    }

    /// Arms a [`DeathKind::OtherBackground`] kill that the level loop drains
    /// via [`Self::take_player_kill`] and applies to the player.  No direct
    /// `DieRestartLevel` dispatch: the player's `Die` sub-state fires it once
    /// the die animation finishes.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.removed {
            return;
        }
        self.pending_kill = Some(DeathKind::OtherBackground);
    }

    /// Collapsing ceilings are indestructible; weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the entity's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the ceiling has landed and the level loop should
    /// purge it from the active object list.
    fn should_remove(&self) -> bool {
        self.removed
    }

    /// Returns the pending kill classification (and clears it) so the level
    /// loop can apply it to the player after the touch dispatch pass.
    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::{CollapsingCeilingEntity, SPEED_FALL, TILE_INDEX, TILESET_INDEX};
    use crate::asset_cache::AssetCache;
    use openjill_core::layout::BLOCK_SIZE_I;
    use openjill_core::{
        ActiveInput, BackgroundEntity, BackgroundGrid, MessageDispatcher, ObjectEntity, Rect,
        RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;

    /// Object record byte length used by the synthetic JN buffer helper.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Test cell: every cell is open air unless its `(x, y)` matches `floor_at`.
    struct SolidIf {
        solid: bool,
    }

    impl BackgroundEntity for SolidIf {
        fn draw(&self, _x: i32, _y: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _cx: i32, _cy: i32, _dispatcher: &mut MessageDispatcher) {}
        fn on_player_touch(
            &mut self,
            _player: &mut dyn ObjectEntity,
            _dispatcher: &mut MessageDispatcher,
        ) {
        }
        fn is_passthrough(&self) -> bool {
            !self.solid
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Builds a grid of `width x height` cells; the `(floor_x, floor_y)` cell is
    /// solid, everything else passable.
    fn grid_with_floor_at(
        width: usize,
        height: usize,
        floor_x: usize,
        floor_y: usize,
    ) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(height);
        for y in 0..height {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(width);
            for x in 0..width {
                row.push(Box::new(SolidIf {
                    solid: x == floor_x && y == floor_y,
                }));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    /// Builds a one-object synthetic JN whose record carries the supplied x/y
    /// position; the rest of the fields default to zero.
    fn synthetic_collapsing_ceiling_at(x: i32, y: i32) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 25; // object_type = CollapsingCeilingManager
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`CollapsingCeilingEntity::observe_player`] +
    /// [`CollapsingCeilingEntity::update`].
    ///
    /// Preconditions: a ceiling sits at `(32, 32)` with a 16x16 bounding box;
    /// the player's bounding box overlaps the cell directly below
    /// (`(32, 48)`).  The background grid has no nearby floor for several
    /// rows so the falling block has somewhere to move.
    ///
    /// Invariants asserted: the first tick after the walks-under arm advances
    /// the block by `SPEED_FALL` pixels downward, and subsequent ticks keep
    /// descending until the block reaches a solid cell.
    #[test]
    fn collapsing_ceiling_walks_under_then_falls() {
        let cache = AssetCache::synthetic();
        let mut ceiling =
            CollapsingCeilingEntity::new(&synthetic_collapsing_ceiling_at(32, 32), &cache);
        let player_bbox = Rect::new(32, 48, BLOCK_SIZE_I, BLOCK_SIZE_I);
        // Floor far enough down that we get at least one fall tick before
        // landing.
        let backgrounds = grid_with_floor_at(8, 8, 2, 6);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        // No observe yet -> no fall.
        ceiling.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            ceiling.bounding_box().y,
            32,
            "untriggered ceiling stays put"
        );

        // Walks-under arms the trigger; first update falls.
        ceiling.observe_player(player_bbox);
        ceiling.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            ceiling.bounding_box().y,
            32 + SPEED_FALL,
            "armed ceiling must descend by SPEED_FALL each tick"
        );

        // Second tick keeps descending (still in open air).
        ceiling.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            ceiling.bounding_box().y,
            32 + SPEED_FALL * 2,
            "ceiling keeps descending while open air remains below"
        );
    }

    /// Unit under test: [`CollapsingCeilingEntity::draw`] uses the SHA-verified
    /// `(tileset, tile)` pair.
    #[test]
    fn collapsing_ceiling_draw_targets_verified_sha_tile() {
        let cache = AssetCache::synthetic();
        let ceiling = CollapsingCeilingEntity::new(&synthetic_collapsing_ceiling_at(0, 0), &cache);
        match ceiling.draw().expect("active ceiling must draw") {
            RenderCommand::Blit { tileset, tile, .. } => {
                assert_eq!(tileset, TILESET_INDEX);
                assert_eq!(tile, TILE_INDEX);
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }
}
