//! Eyes object (JN object type 64).
//!
//! Mirrors `org.jill.game.entities.obj.EyesManager`.  The eyes are a
//! **stationary, non-damaging, non-killable** decoration: a fixed eye sprite
//! with a lens/pupil drawn on top whose offset tracks the player's position
//! (a ray projected from the lens origin toward the player, clamped per axis).
//! Java `EyesManager` extends `AbstractParameterObjectEntity` (not the
//! hit-player base) and defines no `msgKill`, so the eyes never harm the player
//! and cannot be destroyed.
//!
//! Config from `object_conf.json` (`EyesManager`): `tileSet = 62`,
//! `eyesTile = 0`, `lensTile = 1`, `lensOriginX = 5`, `lensOriginY = 4`,
//! `raySize = 4`, `maxMoveY = 1`, `maxMoveXleft = 3`, `maxMoveXright = 2`.

use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::sprite_dims;
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the eyes + lens tiles (`tileSet = 62`).
const TILESET_INDEX: u8 = 62;
/// Tile index of the eye base sprite (`eyesTile = 0`).
const EYES_TILE: u16 = 0;
/// Tile index of the lens/pupil sprite (`lensTile = 1`).
const LENS_TILE: u16 = 1;
/// Lens origin offset within the eye sprite (`lensOriginX`/`lensOriginY`).
const LENS_ORIGIN_X: i32 = 5;
const LENS_ORIGIN_Y: i32 = 4;
/// Length of the projected look-at ray (`raySize = 4`).
const RAY_SIZE: f64 = 4.0;
/// Maximum lens travel per axis (`maxMoveY`, `maxMoveXleft`, `maxMoveXright`).
///
/// Names follow the Java fields verbatim, including their swapped use: the
/// right-facing branch clamps with `maxMoveXleft` and the left-facing branch
/// with `maxMoveXright`.
const MAX_MOVE_Y: i32 = 1;
const MAX_MOVE_X_LEFT: i32 = 3;
const MAX_MOVE_X_RIGHT: i32 = 2;

pub struct EyesEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Last observed player top-left, used to aim the lens.
    player_x: i32,
    player_y: i32,
    /// Current lens offset relative to `(LENS_ORIGIN_X, LENS_ORIGIN_Y)`.
    lens_dx: i32,
    lens_dy: i32,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.
    origin: JnObject,
}

impl EyesEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(TILESET_INDEX, LENS_TILE + 1, "EyesEntity tiles");
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let x = i32::from(item.x());
        let y = i32::from(item.y());
        Self {
            x,
            y,
            w,
            h,
            player_x: x,
            player_y: y,
            lens_dx: 0,
            lens_dy: 0,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for EyesEntity {
    /// Recomputes the lens offset so the pupil points at the player.
    ///
    /// Direct port of `EyesManager.msgUpdate`: project a `raySize`-long ray
    /// from the lens origin toward the player, round to integer pixels, then
    /// clamp and sign each axis.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        let ex = self.x + LENS_ORIGIN_X;
        let ey = self.y + LENS_ORIGIN_Y;
        let rel_x = self.player_x - ex;
        let rel_y = self.player_y - ey;

        let (mut nx, mut ny) = if rel_x == 0 && rel_y == 0 {
            (0, 0)
        } else {
            // `atan(|relY| / |relX|)`; `relX == 0` yields `inf`, whose atan is
            // pi/2 (lens points straight up/down) - matching the Java double
            // arithmetic without a special case.
            let corner = (f64::from(rel_y.abs()) / f64::from(rel_x.abs())).atan();
            let dx = (RAY_SIZE * corner.cos()).round() as i32;
            let dy = (RAY_SIZE * corner.sin()).round() as i32;
            (dx, dy)
        };

        ny = ny.min(MAX_MOVE_Y);
        if rel_y < 0 {
            ny = -ny;
        }

        if rel_x < 0 {
            nx = nx.min(MAX_MOVE_X_RIGHT);
            nx = -nx;
        } else if nx > 3 {
            nx = nx.min(MAX_MOVE_X_LEFT);
        }

        self.lens_dx = nx;
        self.lens_dy = ny;
    }

    /// Returns the eye base sprite (the lens is added by [`Self::draw_multi`]).
    fn draw(&self) -> Option<RenderCommand> {
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: EYES_TILE,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    /// Draws the eye base plus the lens at its tracked offset.
    fn draw_multi(&self) -> Vec<RenderCommand> {
        vec![
            RenderCommand::Blit {
                tileset: TILESET_INDEX,
                tile: EYES_TILE,
                x: self.x,
                y: self.y,
                opaque: false,
                clip: None,
            },
            RenderCommand::Blit {
                tileset: TILESET_INDEX,
                tile: LENS_TILE,
                x: self.x + LENS_ORIGIN_X + self.lens_dx,
                y: self.y + LENS_ORIGIN_Y + self.lens_dy,
                opaque: false,
                clip: None,
            },
        ]
    }

    /// No-op: the eyes do not harm the player (decorative entity).
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: the eyes cannot be killed.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Records the player's top-left so the lens can aim at it next tick.
    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
        self.player_y = player_bbox.y;
    }

    /// Snapshots the decorative eyes for a save game (always persisted).
    ///
    /// The lens aim re-derives from the player position on the next tick, so
    /// only the authored record (position) is persisted.
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_data::jn::JnFile;

    /// Builds an `EyesEntity` at `(x, y)` from a synthetic one-object JN buffer.
    fn make_eyes(x: i32, y: i32) -> EyesEntity {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 64; // object type: eyes
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        let cache = AssetCache::synthetic();
        EyesEntity::new(&jn.objects()[0], &cache)
    }

    fn tick(eyes: &mut EyesEntity) {
        let grid = BackgroundGrid::new(Vec::new());
        eyes.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            &grid,
            &mut MessageDispatcher::new(),
        );
    }

    /// The eyes never move; only the lens offset changes with the player.
    #[test]
    fn eyes_stay_put_and_track_player_both_axes() {
        let mut eyes = make_eyes(100, 100);

        // Player to the lower-right: lens moves right (+x) and down (+y).
        eyes.observe_player(Rect::new(400, 400, 16, 16));
        tick(&mut eyes);
        assert_eq!(eyes.bounding_box().x, 100, "eyes do not move horizontally");
        assert_eq!(eyes.bounding_box().y, 100, "eyes do not move vertically");
        assert!(eyes.lens_dx > 0, "lens aims right toward the player");
        assert!(eyes.lens_dy > 0, "lens aims down toward the player");

        // Player to the upper-left: lens flips to negative on both axes.
        eyes.observe_player(Rect::new(0, 0, 16, 16));
        tick(&mut eyes);
        assert!(eyes.lens_dx < 0, "lens aims left toward the player");
        assert!(eyes.lens_dy < 0, "lens aims up toward the player");
    }

    /// Lens travel is clamped to the configured per-axis maxima.
    #[test]
    fn lens_offset_is_clamped() {
        let mut eyes = make_eyes(100, 100);
        eyes.observe_player(Rect::new(9000, 9000, 16, 16));
        tick(&mut eyes);
        assert!(eyes.lens_dx <= MAX_MOVE_X_LEFT);
        assert!(eyes.lens_dy <= MAX_MOVE_Y);

        eyes.observe_player(Rect::new(0, 9000, 16, 16));
        tick(&mut eyes);
        assert!(eyes.lens_dx >= -MAX_MOVE_X_RIGHT, "left travel clamped");
    }
}
