//! Spark hazard entity (JN object type 65, "Sparks (monsters)").
//!
//! Mirrors `org.jill.game.entities.obj.SparkManager`: an electric spark that
//! slides up and down a vine column between the column's vine extents, harming
//! the player on contact and shoving them one block to the left.  It is a
//! persistent environmental hazard (no lifetime) and cannot be killed.
//!
//! Config from `object_conf.json` (`SparkManager`): `tileSet = 61`,
//! `tile = 0`, `numberTileSet = 4`.

use openjill_core::layout::{BLOCK_SIZE_I, ZAPHOLD_AFTER_TOUCH};
use openjill_core::{
    ActiveInput, BackgroundEntity, BackgroundGrid, DeathKind, MessageDispatcher, MessagePayload,
    MessageType, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::jn::JnObject;

use super::enemy_shared::sprite_dims;
use crate::asset_cache::AssetCache;

/// SHA tileset that owns the spark frames (`tileSet = 61`).
const TILESET_INDEX: u8 = 61;
/// Base tile index of the spark animation (`tile = 0`).
const TILE_BASE: u16 = 0;
/// Number of animation frames (`numberTileSet = 4`).
const NUMBER_TILE_SET: u16 = 4;
/// Vertical travel speed used when the JN record carries none.
const DEFAULT_SPEED: i32 = 2;

/// Returns `true` when cell `(cx, cy)` is a vine (climbable) cell; cells
/// outside the grid count as non-vine boundaries.
fn is_vine_cell(backgrounds: &BackgroundGrid, cx: i32, cy: i32) -> bool {
    if cx < 0 || cy < 0 {
        return false;
    }
    backgrounds
        .get(cx as usize, cy as usize)
        .map(BackgroundEntity::is_climbable)
        .unwrap_or(false)
}

pub struct SparkEntity {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Vertical travel speed (negative up, positive down).
    y_speed: i32,
    /// `(maxYTop, maxYBottom)` travel bounds in pixels, computed lazily on the
    /// first tick once the background grid is available.
    bounds: Option<(i32, i32)>,
    counter: i32,
    zaphold: i32,
    pending_kill: Option<DeathKind>,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live state written back.
    origin: JnObject,
}

impl SparkEntity {
    /// Builds a spark entity from a JN object record.
    pub fn new(item: &JnObject, cache: &AssetCache) -> Self {
        let (w, h) = sprite_dims(cache, TILESET_INDEX);
        let yd = i32::from(item.y_speed());
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            y_speed: if yd != 0 { yd } else { -DEFAULT_SPEED },
            bounds: None,
            counter: i32::from(item.counter()),
            zaphold: i32::from(item.zap_hold()),
            pending_kill: None,
            origin: item.clone(),
        }
    }

    /// Scans the spark's column for the vine extents and returns the pixel
    /// travel bounds `(maxYTop, maxYBottom)` (port of `SparkManager.init`).
    fn compute_bounds(&self, backgrounds: &BackgroundGrid) -> (i32, i32) {
        let block_x = self.x.div_euclid(BLOCK_SIZE_I);
        let block_y_top = self.y.div_euclid(BLOCK_SIZE_I);
        let block_y_bottom = (self.y + self.h).div_euclid(BLOCK_SIZE_I);
        let end_y = backgrounds.height as i32;

        let mut start_y = 0;
        let mut stop_y = end_y - 1;

        // First non-vine cell going down marks the bottom extent.
        let mut idx = block_y_bottom;
        while idx < end_y {
            if !is_vine_cell(backgrounds, block_x, idx) {
                stop_y = idx;
                break;
            }
            idx += 1;
        }

        // First non-vine cell going up marks the top extent.
        let mut idx = block_y_top;
        while idx > -1 {
            if !is_vine_cell(backgrounds, block_x, idx) {
                start_y = idx + 1;
                break;
            }
            idx -= 1;
        }

        let half = self.h / 2;
        (start_y * BLOCK_SIZE_I - half, stop_y * BLOCK_SIZE_I - half)
    }
}

impl ObjectEntity for SparkEntity {
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
        let (max_y_top, max_y_bottom) = match self.bounds {
            Some(b) => b,
            None => {
                let b = self.compute_bounds(backgrounds);
                self.bounds = Some(b);
                b
            }
        };

        if self.zaphold > 0 {
            self.zaphold -= 1;
        }

        self.counter = (self.counter + 1) % i32::from(NUMBER_TILE_SET);

        // Oscillate between the two extents (Java `SparkManager.msgUpdate`).
        if (self.y > max_y_top && self.y_speed < 0) || (self.y < max_y_bottom && self.y_speed > 0) {
            self.y += self.y_speed;
        } else {
            self.y_speed = -self.y_speed;
        }
    }

    fn draw(&self) -> Option<RenderCommand> {
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

    /// On contact, shove the player one block left and damage them
    /// (Java `SparkManager.msgTouch`).
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.zaphold > 0 {
            return;
        }
        self.zaphold = ZAPHOLD_AFTER_TOUCH as i32;
        dispatcher.send(
            MessageType::PlayerMove,
            MessagePayload::Move(-BLOCK_SIZE_I, 0),
        );
        self.pending_kill = Some(DeathKind::Enemy);
    }

    /// No-op: sparks are environmental hazards and cannot be killed.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Snapshots the live spark for a save game (always persisted).
    ///
    /// Writes back position, vertical travel speed, the animation `counter`,
    /// and `zap_hold`; the lazily-computed travel bounds re-derive on restore.
    fn snapshot(&self) -> Option<JnObject> {
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        // `new()` defaults only an authored y_speed of 0 to the upward speed;
        // collapse back to 0 in that one case so the round-trip is exact, but
        // always persist a live speed otherwise (an authored downward spark
        // that reversed up to the default must keep its live upward direction).
        let ys = if obj.y_speed() == 0 && self.y_speed == -DEFAULT_SPEED {
            0
        } else {
            self.y_speed as i16
        };
        obj.set_speed(obj.x_speed(), ys);
        obj.set_counter(self.counter as i16);
        obj.set_zap_hold(self.zaphold as u16);
        Some(obj)
    }

    fn take_player_kill(&mut self) -> Option<DeathKind> {
        self.pending_kill.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy)]
    enum Cell {
        Air,
        Vine,
    }

    struct TestCell(Cell);
    impl BackgroundEntity for TestCell {
        fn draw(&self, _: i32, _: i32) -> Option<RenderCommand> {
            None
        }
        fn update(&mut self, _: i32, _: i32, _: &mut MessageDispatcher) {}
        fn on_player_touch(&mut self, _: &mut dyn ObjectEntity, _: &mut MessageDispatcher) {}
        fn is_passthrough(&self) -> bool {
            true
        }
        fn is_climbable(&self) -> bool {
            matches!(self.0, Cell::Vine)
        }
        fn is_stair(&self) -> bool {
            false
        }
    }

    /// Column `x = 0` is a vine between rows 1..=4; everything else is air.
    fn vine_column_grid() -> BackgroundGrid {
        let (w, h) = (2, 6);
        let mut rows: Vec<Vec<Box<dyn BackgroundEntity>>> = Vec::with_capacity(h);
        for y in 0..h {
            let mut row: Vec<Box<dyn BackgroundEntity>> = Vec::with_capacity(w);
            for x in 0..w {
                let kind = if x == 0 && (1..=4).contains(&y) {
                    Cell::Vine
                } else {
                    Cell::Air
                };
                row.push(Box::new(TestCell(kind)));
            }
            rows.push(row);
        }
        BackgroundGrid::new(rows)
    }

    fn spark_at(x: i32, y: i32, yd: i32) -> SparkEntity {
        SparkEntity {
            x,
            y,
            w: 16,
            h: 16,
            y_speed: yd,
            bounds: None,
            counter: 0,
            zaphold: 0,
            pending_kill: None,
            origin: openjill_data::jn::JnObject::spawned(65, x as u16, y as u16, 16, 16),
        }
    }

    fn tick(spark: &mut SparkEntity, g: &BackgroundGrid) {
        spark.update(
            &ActiveInput::default(),
            &RuntimeState::new(),
            g,
            &mut MessageDispatcher::new(),
        );
    }

    /// The spark oscillates within the vine column and reverses at the extents
    /// rather than drifting away forever.
    #[test]
    fn spark_oscillates_within_vine_extents() {
        let g = vine_column_grid();
        // Start mid-column moving up.
        let mut spark = spark_at(0, 48, -2);
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        let mut reversed_up = false;
        let mut reversed_down = false;
        let mut prev = spark.y_speed;
        for _ in 0..400 {
            tick(&mut spark, &g);
            min_y = min_y.min(spark.y);
            max_y = max_y.max(spark.y);
            if prev < 0 && spark.y_speed > 0 {
                reversed_down = true;
            }
            if prev > 0 && spark.y_speed < 0 {
                reversed_up = true;
            }
            prev = spark.y_speed;
        }
        assert!(
            reversed_up && reversed_down,
            "spark reverses at both extents"
        );
        // Stays within a bounded band (does not escape the column).
        assert!(max_y - min_y <= 6 * BLOCK_SIZE_I, "spark travel is bounded");
    }

    /// On contact the spark shoves the player left a block and arms a kill.
    #[test]
    fn spark_shoves_and_damages_on_touch() {
        use openjill_core::MessageHandler;
        use std::sync::{Arc, Mutex};

        struct Rec(Arc<Mutex<Vec<(i32, i32)>>>);
        impl MessageHandler for Rec {
            fn handle(&mut self, _: MessageType, p: &MessagePayload) {
                if let MessagePayload::Move(dx, dy) = p {
                    self.0.lock().unwrap().push((*dx, *dy));
                }
            }
        }

        let moves = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(MessageType::PlayerMove, Box::new(Rec(moves.clone())));

        let mut spark = spark_at(0, 48, -2);
        spark.on_touch(&RuntimeState::new(), &mut dispatcher);

        assert_eq!(
            moves.lock().unwrap().as_slice(),
            &[(-BLOCK_SIZE_I, 0)],
            "player is shoved one block left"
        );
        assert_eq!(spark.take_player_kill(), Some(DeathKind::Enemy));
    }
}
