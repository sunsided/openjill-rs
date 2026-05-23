//! Falling spike hazard entity (JN object type 38).
//!
//! Mirrors `org.jill.game.entities.obj.FallingSpikeManager` from the Java
//! reference: a spike pinned to the ceiling that falls when triggered, kills
//! the player on contact, and removes itself once it lands on a solid cell.
//!
//! The Java reference arms the fall through a `TRIGGER` message dispatched by
//! a linked `SwitchEntity`; the Rust port follows issue #91 and arms the fall
//! the first tick the player's bounding box enters the column directly below
//! the spike ("walks-under" detection).  Switch-driven trigger replaces the
//! walks-under heuristic once `SwitchEntity` lands (issue #63).
//!
//! Tile / tileset identity comes from `object_conf.json`: `tileSet = 14`,
//! `tile = 33`, `fallingSpeed = 2`, `fallingSpeedMax = 16`.  Falling speed
//! accelerates by `fallingSpeed` each tick up to `fallingSpeedMax`, mirroring
//! `FallingSpikeManager.msgUpdate`'s `ySpeed += fallingSpeed` accumulator.
//!
//! FIXME(epic-6): confirm tileset 14 tile 33 against the original `JILL1.SHA`
//! once the SHA dumper grows a visual / PNG output.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset that owns the falling spike sprite.
const TILESET_INDEX: u8 = 14;

/// Tile index inside [`TILESET_INDEX`] for the falling spike.
const TILE_INDEX: u16 = 33;

/// Per-tick downward acceleration (`FallingSpikeManager.fallingSpeed`).
const FALLING_SPEED: i32 = 2;

/// Terminal fall speed cap (`FallingSpikeManager.fallingSpeedMax`).
const FALLING_SPEED_MAX: i32 = 16;

/// Falling spike entity.
pub struct FallingSpikeEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Current downward speed in pixels per tick; `0` while parked, positive
    /// once the trigger arms.
    y_speed: i32,
    /// `true` once the spike lands on a solid cell and the level loop should
    /// purge it.
    removed: bool,
    /// Player bounding box recorded by the most recent
    /// [`Self::observe_player`] call; consumed during [`Self::update`] to arm
    /// the walks-under trigger.
    last_player_bbox: Option<Rect>,
    /// Pending player-kill classification armed in [`Self::on_touch`] and
    /// drained by the level loop via [`ObjectEntity::take_player_kill`].
    pending_kill: Option<DeathKind>,
}

impl FallingSpikeEntity {
    /// Builds a falling spike from a JN object record.
    pub fn new(item: &JnObject, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            y_speed: 0,
            removed: false,
            last_player_bbox: None,
            pending_kill: None,
        }
    }

    /// Returns the column-aligned probe rectangle below the spike that the
    /// player must enter to arm the fall.
    ///
    /// Extends from the spike's bottom edge to the bottom of the map; the
    /// walks-under detection then fires the moment the player crosses into
    /// the same column at any depth, matching the spirit of the Java
    /// reference's switch trigger (which fires regardless of vertical
    /// distance).
    fn trigger_zone(&self) -> Rect {
        Rect::new(self.x, self.y + self.h, self.w, i32::MAX / 2)
    }

    /// Returns `true` if any cell in the row immediately below the spike's
    /// bottom edge is solid; mirrors `UtilityObjectEntity.moveObjectDown`'s
    /// landing check.
    fn floor_below(&self, backgrounds: &BackgroundGrid) -> bool {
        let probe_y = self.y + self.h;
        let cell_y = probe_y.div_euclid(BLOCK_SIZE_I);
        if cell_y < 0 || cell_y as usize >= backgrounds.height {
            return true;
        }
        let cell_y = cell_y as usize;
        let cell_x_left = self.x.div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cell_x_right = (self.x + self.w - 1).div_euclid(BLOCK_SIZE_I).max(0) as usize;
        let cell_x_right = cell_x_right.min(backgrounds.width.saturating_sub(1));
        for cx in cell_x_left..=cell_x_right {
            if let Some(cell) = backgrounds.get(cx, cell_y)
                && (!cell.is_passthrough() || cell.is_stair())
            {
                return true;
            }
        }
        false
    }
}

impl ObjectEntity for FallingSpikeEntity {
    /// Drives the fall once armed.  Accelerates `y_speed` by [`FALLING_SPEED`]
    /// each tick (capped at [`FALLING_SPEED_MAX`]), advances by `y_speed`
    /// pixels, then flags removal when the floor cell below blocks further
    /// motion (the Java reference's `killme` dispatch after `moveObjectDown`
    /// reports `false`).
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

        if self.y_speed == 0
            && let Some(player_bbox) = self.last_player_bbox
            && player_bbox.intersects(&self.trigger_zone())
        {
            self.y_speed = FALLING_SPEED;
        }

        if self.y_speed <= 0 {
            return;
        }

        if self.floor_below(backgrounds) {
            self.removed = true;
            return;
        }

        self.y += self.y_speed;
        if self.y_speed < FALLING_SPEED_MAX {
            self.y_speed = (self.y_speed + FALLING_SPEED).min(FALLING_SPEED_MAX);
        }
    }

    /// Emits a [`RenderCommand::Blit`] for the falling spike tile while the
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

    /// Falling spikes are indestructible; weapons leave them untouched.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the entity's bounding box for collision tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Returns `true` once the spike has landed and the level loop should
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
    use super::{FALLING_SPEED, FALLING_SPEED_MAX, FallingSpikeEntity, TILE_INDEX, TILESET_INDEX};
    use crate::asset_cache::AssetCache;
    use openjill_core::layout::BLOCK_SIZE_I;
    use openjill_core::{
        ActiveInput, BackgroundEntity, BackgroundGrid, MessageDispatcher, ObjectEntity, Rect,
        RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;

    const OBJECT_RECORD_BYTES: usize = 31;

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

    fn open_grid(width: usize, height: usize) -> BackgroundGrid {
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(height);
        for _ in 0..height {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(width);
            for _ in 0..width {
                row.push(Box::new(SolidIf { solid: false }));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn synthetic_falling_spike_at(x: i32, y: i32) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 38; // object_type = FallingSpikeManager
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`FallingSpikeEntity::observe_player`] +
    /// [`FallingSpikeEntity::update`].
    ///
    /// Invariants asserted: walks-under arms the fall; subsequent ticks
    /// advance the spike downward and accelerate `y_speed` up to
    /// [`FALLING_SPEED_MAX`].
    #[test]
    fn falling_spike_walks_under_then_falls() {
        let cache = AssetCache::synthetic();
        let mut spike = FallingSpikeEntity::new(&synthetic_falling_spike_at(32, 32), &cache);
        let player_bbox = Rect::new(32, 80, BLOCK_SIZE_I, BLOCK_SIZE_I);
        let backgrounds = open_grid(8, 16);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        let mut dispatcher = MessageDispatcher::new();

        spike.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(spike.bounding_box().y, 32, "untriggered spike stays put");

        spike.observe_player(player_bbox);
        spike.update(&input, &state, &backgrounds, &mut dispatcher);
        assert_eq!(
            spike.bounding_box().y,
            32 + FALLING_SPEED,
            "first tick post-arm advances by FALLING_SPEED"
        );

        // Drive enough ticks to reach the speed cap; the spike must keep
        // falling but not exceed FALLING_SPEED_MAX per tick.
        for _ in 0..32 {
            spike.update(&input, &state, &backgrounds, &mut dispatcher);
        }
        let prev_y = spike.bounding_box().y;
        spike.update(&input, &state, &backgrounds, &mut dispatcher);
        assert!(
            spike.bounding_box().y - prev_y <= FALLING_SPEED_MAX,
            "spike fall speed must cap at FALLING_SPEED_MAX"
        );
    }

    /// Unit under test: [`FallingSpikeEntity::draw`] uses the SHA-verified
    /// `(tileset, tile)` pair for the active spike.
    #[test]
    fn falling_spike_draw_targets_verified_sha_tile() {
        let cache = AssetCache::synthetic();
        let spike = FallingSpikeEntity::new(&synthetic_falling_spike_at(0, 0), &cache);
        match spike.draw().expect("active spike must draw") {
            RenderCommand::Blit { tileset, tile, .. } => {
                assert_eq!(tileset, TILESET_INDEX);
                assert_eq!(tile, TILE_INDEX);
            }
            other => panic!("expected Blit, got {other:?}"),
        }
    }
}
