//! Skull object (JN object type 51).
//!
//! Mirrors `org.jill.game.entities.obj.SkullManager`: a **stationary, harmless,
//! non-killable** wall skull.  It shows a fixed idle tile until a `TRIGGER`
//! whose link id matches its own (`switch.counter == skull.counter`) arrives,
//! after which it loops an eye-rolling animation forever.  Java `SkullManager`
//! extends `AbstractParameterObjectEntity` (not the hit-player base) and never
//! moves or harms the player.
//!
//! Config from `object_conf.json`: `tileSet = 47`, skull tiles `0..=skullMax`
//! (`skullMax = 2`), eye tiles `3..=7` drawn at `(eyeLeftX=0, eyeLeftY=5)` and
//! `(eyeRightX=10, eyeRightY=6)`, `fixedTile = 0`.  The animation has
//! `numberTileSet * 2 = 8` frames.

use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::sprite_dims;
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the skull + eye tiles (`tileSet = 47`).
const TILESET_INDEX: u8 = 47;
/// Idle tile shown before the skull is triggered (`fixedTile = 0`).
const FIXED_TILE: u16 = 0;
/// Number of animation frames (`numberTileSet * 2 = 8`).
const FRAMES: usize = 8;
/// Lens draw offsets within the skull sprite.
const EYE_LEFT_X: i32 = 0;
const EYE_LEFT_Y: i32 = 5;
const EYE_RIGHT_X: i32 = 10;
const EYE_RIGHT_Y: i32 = 6;

/// Per-frame skull base tile: ping-pong over `0..=skullMax(2)`, each tile held
/// for two frames (Java `loadSkullImage`).
const SKULL_FRAMES: [u16; FRAMES] = [0, 0, 1, 1, 2, 2, 1, 1];
/// Per-frame left-eye tile: ping-pong over `3..=7` starting at `eyeLeftStart=3`.
const LEFT_EYE_FRAMES: [u16; FRAMES] = [3, 4, 5, 6, 7, 6, 5, 4];
/// Per-frame right-eye tile: ping-pong over `3..=7` starting at
/// `eyeRightStart=7`.
const RIGHT_EYE_FRAMES: [u16; FRAMES] = [7, 6, 5, 4, 3, 4, 5, 6];

pub struct SkullEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Trigger link id (JN `counter`); the skull activates when a matching
    /// `TRIGGER` arrives.
    link_id: i32,
    /// `false` until triggered; once `true` the eye animation loops.
    active: bool,
    /// Animation frame index `0..FRAMES` while active.
    frame: usize,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.  The
    /// trigger link id lives in the authored `counter`, preserved untouched.
    origin: JnObject,
}

impl SkullEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(TILESET_INDEX, 8, "SkullEntity tiles");
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            link_id: i32::from(item.counter()),
            active: false,
            frame: 0,
            origin: item.clone(),
        }
    }
}

impl ObjectEntity for SkullEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        // Once triggered, cycle the eye animation forever (Java `msgUpdate`).
        if self.active {
            self.frame = (self.frame + 1) % FRAMES;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        let tile = if self.active {
            SKULL_FRAMES[self.frame]
        } else {
            FIXED_TILE
        };
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn draw_multi(&self) -> Vec<RenderCommand> {
        // Idle: just the fixed skull tile.
        if !self.active {
            return self.draw().into_iter().collect();
        }
        // Active: skull base plus the two animated eyes.
        let blit = |tile: u16, x: i32, y: i32| RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile,
            x,
            y,
            opaque: false,
            clip: None,
        };
        vec![
            blit(SKULL_FRAMES[self.frame], self.x, self.y),
            blit(
                LEFT_EYE_FRAMES[self.frame],
                self.x + EYE_LEFT_X,
                self.y + EYE_LEFT_Y,
            ),
            blit(
                RIGHT_EYE_FRAMES[self.frame],
                self.x + EYE_RIGHT_X,
                self.y + EYE_RIGHT_Y,
            ),
        ]
    }

    /// No-op: the skull does not harm the player.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: the skull cannot be killed.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Starts the eye animation when a trigger with the matching link id fires
    /// (Java `recieveMessage` TRIGGER: `switch.counter == this.counter`).
    fn receive_trigger(&mut self, link_id: i32) {
        if link_id == self.link_id {
            self.active = true;
        }
    }

    /// Snapshots the skull for a save game (always persisted).
    ///
    /// The active/animation state re-arms from the linked switch on restore,
    /// so only the authored record (position + link id via `counter`) is
    /// persisted.
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

    fn skull(link_id: i16) -> SkullEntity {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 51; // skull
        // counter field lives at record offset 17 (see ObjectItemImpl layout);
        // set it via the JnObject accessor expectation by writing the bytes.
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN parses");
        let cache = AssetCache::synthetic();
        let mut s = SkullEntity::new(&jn.objects()[0], &cache);
        s.link_id = i32::from(link_id);
        s
    }

    fn tick(s: &mut SkullEntity) {
        let grid = BackgroundGrid::new(Vec::new());
        s.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            &grid,
            &mut MessageDispatcher::new(),
        );
    }

    /// The skull is idle until its matching trigger fires, then animates.
    #[test]
    fn skull_activates_on_matching_trigger() {
        let mut s = skull(5);
        // Idle: frame never advances, draws the fixed tile.
        tick(&mut s);
        assert!(!s.active);
        assert_eq!(s.draw_multi().len(), 1, "idle skull draws one tile");

        // A non-matching trigger does nothing.
        s.receive_trigger(4);
        assert!(!s.active);

        // The matching trigger starts the animation.
        s.receive_trigger(5);
        assert!(s.active);
        let f0 = s.frame;
        tick(&mut s);
        assert_ne!(s.frame, f0, "frame advances once active");
        assert_eq!(
            s.draw_multi().len(),
            3,
            "active skull draws base + two eyes"
        );
    }

    /// The skull never harms the player and cannot be killed.
    #[test]
    fn skull_is_harmless_and_unkillable() {
        let mut s = skull(1);
        s.on_kill(99, DeathKind::Enemy);
        assert_eq!(s.take_player_kill(), None, "skull arms no contact kill");
    }
}
