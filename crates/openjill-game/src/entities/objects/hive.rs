//! Hive enemy entity (JN object type 45).
//!
//! Mirrors `org.jill.game.entities.obj.HiveManager`: a stationary, weapon-
//! killable spawner.  While idle it has a small per-tick chance
//! (`1 / maxRandomValue`) to begin a charge cycle; the charge advances one
//! step every `counterMaxWait` ticks and, once it passes `counterCreateBees`,
//! spawns a single bee just to the player-facing side of the hive and returns
//! to idle.  It faces the player (used for the spawn side).
//!
//! Config from `object_conf.json`: `counterCreateBees = 2`,
//! `maxRandomValue = 20`, `counterMaxWait = 3`, `beesObject = BeesManager`
//! (spawned as object type 46).
//!
//! Tileset/tile: the Rust port renders from tileset 8 (the Java reference uses
//! `tileSet = 37`); render tiles are not reconciled here, only behaviour.
//! FIXME(epic-6): reconcile the hive render tileset against JILL1.SHA.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload, MessageType,
    ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::EnemyRng;
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the hive frames (Rust port choice).
const TILESET_INDEX: u8 = 8;
/// Base tile index within [`TILESET_INDEX`].
const TILE_BASE: u16 = 8;
/// Number of rendered animation frames.
const NUMBER_TILE_SET: u16 = 2;
/// Score awarded when the hive is killed.
const SCORE_VALUE: i32 = 500;
/// Charge value past which a bee is spawned (`counterCreateBees = 2`).
const COUNTER_CREATE_BEES: i32 = 2;
/// Idle re-roll denominator: `1 / maxRandomValue` chance per tick to begin a
/// charge cycle (`maxRandomValue = 20`).
const MAX_RANDOM_VALUE: i32 = 20;
/// Ticks to wait per charge step (`counterMaxWait = 3`).
const COUNTER_MAX_WAIT: i32 = 3;
/// Object type spawned (BeesManager).
const BEES_OBJECT_TYPE: u8 = 46;

pub struct HiveEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Charge counter: `0` = idle, `1..=counterCreateBees` = charging.
    counter: i32,
    /// Ticks waited at the current charge step.
    counter_wait: i32,
    /// Player-facing sign (`>= 0` right, `< 0` left); chooses the spawn side.
    facing: i32,
    player_x: i32,
    dead: bool,
    score_dispatched: bool,
    zaphold: i32,
    rng: EnemyRng,
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
        let x = i32::from(item.x());
        let y = i32::from(item.y());
        Self {
            x,
            y,
            w,
            h,
            counter: 0,
            counter_wait: 0,
            facing: 1,
            player_x: x,
            dead: false,
            score_dispatched: false,
            zaphold: 0,
            rng: EnemyRng::new((x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32)),
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

        // Face the player (chooses which side a spawned bee appears on).
        let xd = self.player_x - self.x;
        if xd != 0 {
            self.facing = xd.signum();
        }

        if self.counter == 0 {
            // Idle: small random chance to begin a charge cycle.
            if self.rng.range(0, MAX_RANDOM_VALUE) == 0 {
                self.counter = 1;
            }
        } else if self.counter_wait < COUNTER_MAX_WAIT {
            self.counter_wait += 1;
        } else {
            self.counter += 1;
            self.counter_wait = 0;
            if self.counter > COUNTER_CREATE_BEES {
                self.counter = 0;
                // Spawn one bee on the player-facing side of the hive.
                let bee_x = if self.facing >= 0 {
                    self.x + self.w / 2
                } else {
                    self.x - self.w / 2
                };
                dispatcher.send(
                    MessageType::CreateObject,
                    MessagePayload::SpawnAt {
                        object_type: BEES_OBJECT_TYPE,
                        x: bee_x,
                        y: self.y,
                        xd: 0,
                        yd: 0,
                    },
                );
            }
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
        if self.dead {
            return None;
        }
        // Frame reflects the charge level (idle = frame 0).
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

    fn observe_player(&mut self, player_bbox: Rect) {
        self.player_x = player_bbox.x;
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

    /// Over many ticks the hive eventually spawns bees, each requesting
    /// `object_type = 46` on the player-facing side.
    #[test]
    fn hive_spawns_bees_toward_player_over_time() {
        struct Recorder(Arc<Mutex<Vec<MessagePayload>>>);
        impl MessageHandler for Recorder {
            fn handle(&mut self, _: MessageType, payload: &MessagePayload) {
                self.0.lock().unwrap().push(payload.clone());
            }
        }

        let cache = AssetCache::synthetic();
        let mut hive = HiveEntity::new(&synthetic_hive(100, 32), &cache);

        let payloads: Arc<Mutex<Vec<MessagePayload>>> = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::CreateObject,
            Box::new(Recorder(Arc::clone(&payloads))),
        );

        let grid = empty_grid(8, 8);
        let input = ActiveInput::default();
        let state = RuntimeState::new();
        // Player to the right, so bees spawn on the right side (x + w/2).
        for _ in 0..2000 {
            hive.observe_player(Rect::new(400, 32, 16, 16));
            hive.update(&input, &state, &grid, &mut dispatcher);
        }

        let got = payloads.lock().unwrap();
        assert!(!got.is_empty(), "hive eventually spawns bees");
        let expected_x = hive.x + hive.w / 2;
        for p in got.iter() {
            assert!(
                matches!(
                    p,
                    MessagePayload::SpawnAt { object_type: 46, x, .. } if *x == expected_x
                ),
                "each spawn requests a bee on the player-facing side; got {p:?}"
            );
        }
    }
}
