//! Hive enemy entity (JN object type 45).
//!
//! Mirrors `org.jill.game.entities.obj.HiveManager`: stationary spawner that
//! periodically dispatches `CreateObject` to spawn `BeesEntity` (type 46).
//! Killed by weapons; does not kill the player on contact.
//!
//! Tileset/tile: `tileSet = 8`, `tile = 8`, `numberTileSet = 2`.
//! FIXME(epic-6): confirm tileset 8 tiles 8..=9 against JILL1.SHA dump.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// SHA tileset that owns the hive frames.
///
/// REVERSE-ENGINEERED: Rust port choice. Note the Java reference's
/// `HiveManager.tileSet = 37`; the Rust port currently uses tileset 8 with
/// [`TILE_BASE`] = 8. Future engine config file should expose this and
/// reconcile with the Java reference.
const TILESET_INDEX: u8 = 8;
/// Base tile index within [`TILESET_INDEX`].
///
/// REVERSE-ENGINEERED: paired with [`TILESET_INDEX`] above.
const TILE_BASE: u16 = 8;
/// Number of animation frames cycled by the hive sprite.
///
/// REVERSE-ENGINEERED: verified at construction by
/// [`AssetCache::assert_tile_subset`].
const NUMBER_TILE_SET: u16 = 2;
/// Score awarded when the hive is killed.
///
/// REVERSE-ENGINEERED: derived from the Java reference's `HiveManager`
/// `point` field.
const SCORE_VALUE: i32 = 500;
/// Ticks between bee spawns.
///
/// REVERSE-ENGINEERED: matches the Java reference's hive spawn cadence.
const SPAWN_PERIOD: i32 = 60;

pub struct HiveEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    spawn_counter: i32,
    counter: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
}

impl HiveEntity {
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        cache.assert_tile_subset(
            TILESET_INDEX,
            TILE_BASE + NUMBER_TILE_SET,
            "HiveEntity NUMBER_TILE_SET",
        );
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            spawn_counter: 0,
            counter: 0,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
        }
    }
}

impl ObjectEntity for HiveEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        dispatcher: &mut MessageDispatcher,
    ) {
        if self.dead {
            if !self.score_dispatched {
                self.score_dispatched = true;
                dispatcher.send(
                    MessageType::InventoryPoint,
                    MessagePayload::Count(SCORE_VALUE),
                );
            }
            return;
        }
        if self.zaphold > 0 {
            self.zaphold -= 1;
        }

        self.spawn_counter += 1;
        if self.spawn_counter >= SPAWN_PERIOD {
            self.spawn_counter = 0;
            dispatcher.send(
                MessageType::CreateObject,
                MessagePayload::SpawnAt {
                    object_type: 46,
                    x: self.x,
                    y: self.y,
                    xd: 0,
                    yd: 0,
                },
            );
        }

        self.counter += 1;
        if self.counter >= NUMBER_TILE_SET as i32 {
            self.counter = 0;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.dead {
            return None;
        }
        let frame = (self.counter as u16).min(NUMBER_TILE_SET - 1);
        Some(RenderCommand::Blit {
            tileset: TILESET_INDEX,
            tile: TILE_BASE + frame,
            x: self.x,
            y: self.y,
            opaque: false,
            clip: None,
        })
    }

    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {
        if self.dead || self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
    }

    fn on_kill(&mut self, damage: i32, _death_kind: DeathKind) {
        if self.dead || damage < 1 {
            return;
        }
        self.dead = true;
    }

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    fn is_dead(&self) -> bool {
        self.dead
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use openjill_core::{
        ActiveInput, BackgroundEntity, BackgroundGrid, MessageDispatcher, MessageHandler,
        MessagePayload, MessageType, ObjectEntity, RenderCommand, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    struct EmptyBg;
    impl BackgroundEntity for EmptyBg {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            true
        }
        fn is_climbable(&self) -> bool {
            false
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    fn empty_grid(w: usize, h: usize) -> BackgroundGrid {
        let rows = (0..h)
            .map(|_| {
                (0..w)
                    .map(|_| Box::new(EmptyBg) as Box<dyn BackgroundEntity>)
                    .collect()
            })
            .collect();
        BackgroundGrid::new(rows)
    }

    fn synthetic_hive(x: i32, y: i32) -> openjill_data::jn::JnObject {
        const OBJECT_RECORD_BYTES: usize = 31;
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 45;
        bytes[record_off + 1..record_off + 3].copy_from_slice(&(x as u16).to_le_bytes());
        bytes[record_off + 3..record_off + 5].copy_from_slice(&(y as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Hive dispatches `CreateObject` with `object_type = 46` after `SPAWN_PERIOD` ticks.
    #[test]
    fn hive_dispatches_bees_create_object_after_spawn_period() {
        struct Recorder(Arc<Mutex<Vec<MessagePayload>>>);
        impl MessageHandler for Recorder {
            fn handle(&mut self, _: MessageType, payload: &MessagePayload) {
                self.0.lock().unwrap().push(payload.clone());
            }
        }

        let cache = AssetCache::synthetic();
        let mut hive = HiveEntity::new(&synthetic_hive(32, 32), &cache);
        let payloads: Arc<Mutex<Vec<MessagePayload>>> = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::CreateObject,
            Box::new(Recorder(Arc::clone(&payloads))),
        );

        let grid = empty_grid(8, 8);
        let input = ActiveInput::default();
        let state = RuntimeState::new();

        for _ in 0..SPAWN_PERIOD {
            hive.update(&input, &state, &grid, &mut dispatcher);
        }

        let got = payloads.lock().unwrap();
        assert_eq!(
            got.len(),
            1,
            "hive spawns exactly one bee after SPAWN_PERIOD ticks"
        );
        assert!(
            matches!(
                got[0],
                MessagePayload::SpawnAt {
                    object_type: 46,
                    ..
                }
            ),
            "spawn payload must request object_type 46 (BeesEntity); got {:?}",
            got[0]
        );
    }
}
