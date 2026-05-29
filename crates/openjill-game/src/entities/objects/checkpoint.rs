//! Checkpoint object entity (JN object type 12).
//!
//! Mirrors a simplified subset of `org.jill.game.entities.obj.CheckPointManager`
//! from the Java reference: when the player overlaps the checkpoint, the
//! entity dispatches a [`MessageType::CheckpointChangeLevel`] message whose
//! payload carries the destination level number (the JN `counter` field) and
//! the matching JN filename (`<counter>.<episode jn ext>`).
//!
//! The Java port distinguishes "music control" string entries (`*`, `#`),
//! "load previous map" entries (`!`), and "destination level" entries (any
//! other content) via the JN string stack.  This port mirrors only the
//! map-return path: a leading `!` in the resolved string entry maps the
//! destination to the world map (`MAP.<jn ext>` filename with
//! [`MAP_LEVEL`](openjill_core::MAP_LEVEL) as the level number).  Music
//! control codes are recognised but otherwise ignored.

use openjill_core::layout::BLOCK_SIZE_I;
use openjill_core::{
    ActiveInput, BackgroundGrid, ChangeLevelPayload, DeathKind, MAP_LEVEL, MessageDispatcher,
    MessagePayload, MessageType, ObjectEntity, Rect, RenderCommand, RuntimeState,
};
use openjill_data::episode;
use openjill_data::jn::JnObject;

use crate::asset_cache::AssetCache;

/// Episode 1 JN filename extension (`"JN1"`).
///
/// Hard-coded against [`episode::JILL1`] for now: the level screen also
/// hard-codes episode 1 (`EPISODE_SAVE_PREFIX` in `screens::level_screen`),
/// and the multi-episode work is scheduled as part of issue 56 follow-ups.
const EPISODE_JN_EXT: &str = episode::JILL1.jn_ext;

/// Destination kind resolved at construction from the checkpoint's JN string
/// entry.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CheckpointDestination {
    /// Transition to a numbered level.
    Level {
        /// Destination level number (copied from the JN `counter` field).
        level_number: i32,
        /// JN filename for the destination level.
        level_file: String,
    },
    /// Transition back to the world map.
    Map {
        /// JN filename for the world map.
        map_file: String,
    },
}

impl CheckpointDestination {
    /// Builds a destination from a JN counter and optional string entry.
    fn from_jn(counter: i16, string_entry: Option<&str>) -> Self {
        if string_entry.map(starts_with_map_marker).unwrap_or(false) {
            Self::Map {
                map_file: format!("MAP.{EPISODE_JN_EXT}"),
            }
        } else {
            let level_number = i32::from(counter);
            Self::Level {
                level_number,
                level_file: format!("{level_number}.{EPISODE_JN_EXT}"),
            }
        }
    }

    /// Converts the destination into the [`ChangeLevelPayload`] that the
    /// checkpoint dispatches on touch.
    fn into_payload(self) -> ChangeLevelPayload {
        match self {
            Self::Level {
                level_number,
                level_file,
            } => ChangeLevelPayload {
                level_file,
                level_number,
            },
            Self::Map { map_file } => ChangeLevelPayload {
                level_file: map_file,
                level_number: MAP_LEVEL,
            },
        }
    }
}

/// Returns `true` when `value` is a Java-reference "load previous map"
/// marker, i.e. starts with `!`.
fn starts_with_map_marker(value: &str) -> bool {
    value.starts_with('!')
}

/// Checkpoint object entity.
///
/// Carries the JN `counter` value alongside position so the per-tick loop can
/// identify the checkpoint via [`ObjectEntity::is_checkpoint`], and stores
/// the pre-resolved destination so `on_touch` can dispatch without
/// re-deriving it.
pub struct CheckPointEntity {
    /// World X position in pixels.
    x: i32,
    /// World Y position in pixels.
    y: i32,
    /// Bounding box width in pixels.
    w: i32,
    /// Bounding box height in pixels.
    h: i32,
    /// Destination level number copied from the JN `counter` field.
    counter: i16,
    /// Pre-resolved transition destination.
    destination: CheckpointDestination,
    /// `true` once the checkpoint has dispatched its transition.
    fired: bool,
    /// The JN object record this entity was built from, re-emitted by
    /// [`ObjectEntity::snapshot`] with the live position written back.  The
    /// authored `counter` and string pointer (which together resolve the
    /// destination) are preserved untouched.
    origin: JnObject,
}

impl CheckPointEntity {
    /// Builds a checkpoint entity from a JN object record and its optional
    /// string-stack entry.
    pub fn new(item: &JnObject, string_entry: Option<&str>, _cache: &AssetCache) -> Self {
        let w = i32::from(item.width()).max(BLOCK_SIZE_I);
        let h = i32::from(item.height()).max(BLOCK_SIZE_I);
        let counter = item.counter();
        Self {
            x: i32::from(item.x()),
            y: i32::from(item.y()),
            w,
            h,
            counter,
            destination: CheckpointDestination::from_jn(counter, string_entry),
            fired: false,
            origin: item.clone(),
        }
    }

    /// Returns the JN `counter` value, used as the destination level number
    /// when this checkpoint fires a transition.
    pub fn counter(&self) -> i16 {
        self.counter
    }
}

impl ObjectEntity for CheckPointEntity {
    /// Checkpoints have no per-tick state once spawned.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Checkpoints render no sprite; the surrounding world tiles already
    /// convey the level entry point.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// Dispatches the pre-resolved
    /// [`MessageType::CheckpointChangeLevel`] payload and flags the
    /// checkpoint for removal so the level loop does not re-fire it on the
    /// following tick.
    fn on_touch(&mut self, _state: &RuntimeState, dispatcher: &mut MessageDispatcher) {
        if self.fired {
            return;
        }
        let payload = self.destination.clone().into_payload();
        dispatcher.send(
            MessageType::CheckpointChangeLevel,
            MessagePayload::ChangeLevel(payload),
        );
        self.fired = true;
    }

    /// Checkpoints are not damaged by weapons.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns the checkpoint's bounding box for player overlap tests.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    /// Snapshots the checkpoint for a save game, or `None` once it has fired
    /// (it is being reaped). The destination re-resolves from the preserved
    /// `counter` and string pointer on restore.
    fn snapshot(&self) -> Option<JnObject> {
        if self.fired {
            return None;
        }
        let mut obj = self.origin.clone();
        obj.set_position(self.x as u16, self.y as u16);
        Some(obj)
    }

    /// Returns `true`: this entity is the level-transition marker.
    fn is_checkpoint(&self) -> bool {
        true
    }

    /// Returns `true` once the checkpoint has dispatched its transition so
    /// the level loop can purge it on the same tick.
    fn should_remove(&self) -> bool {
        self.fired
    }
}

#[cfg(test)]
mod tests {
    use super::CheckPointEntity;
    use crate::asset_cache::AssetCache;
    use openjill_core::{
        ChangeLevelPayload, MAP_LEVEL, MessageDispatcher, MessageHandler, MessagePayload,
        MessageType, ObjectEntity, RuntimeState,
    };
    use openjill_data::jn::JnFile;
    use std::sync::{Arc, Mutex};

    /// Object record byte length used by the synthetic JN buffer helpers.
    const OBJECT_RECORD_BYTES: usize = 31;

    /// Shared recording buffer used by `RecordingHandler` in unit tests.
    type RecordingBuffer = Arc<Mutex<Vec<(MessageType, MessagePayload)>>>;

    /// Test helper: records every delivered message into a shared buffer.
    struct RecordingHandler(RecordingBuffer);

    impl MessageHandler for RecordingHandler {
        /// Appends `(msg_type, payload)` to the recording buffer.
        fn handle(&mut self, msg_type: MessageType, payload: &MessagePayload) {
            self.0.lock().unwrap().push((msg_type, payload.clone()));
        }
    }

    /// Builds a one-object JN file whose record has `object_type = 12` and
    /// the supplied `counter`, returning the inner `JnObject` clone.
    fn synthetic_checkpoint_object(counter: i16) -> openjill_data::jn::JnObject {
        let total = 128 * 64 * 2 + 2 + OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&1u16.to_le_bytes());
        let record_off = count_off + 2;
        bytes[record_off] = 12;
        // `counter` is the eleventh i16 field in the object record (the JN
        // parser places it at byte offset 19 within the record).
        let counter_off = record_off + 19;
        bytes[counter_off..counter_off + 2].copy_from_slice(&counter.to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: [`CheckPointEntity::on_touch`] for a numbered
    /// destination level.
    ///
    /// Preconditions: the checkpoint record carries `counter = 3` with no
    /// string entry; a recording handler is subscribed to
    /// [`MessageType::CheckpointChangeLevel`].
    ///
    /// Invariants asserted: exactly one [`MessagePayload::ChangeLevel`]
    /// message is dispatched with `level_number == 3` and
    /// `level_file == "3.JN1"`; the entity then flags itself for removal.
    #[test]
    fn on_touch_dispatches_change_level_for_numbered_destination() {
        let cache = AssetCache::synthetic();
        let mut checkpoint = CheckPointEntity::new(&synthetic_checkpoint_object(3), None, &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::CheckpointChangeLevel,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        checkpoint.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1, "exactly one transition dispatched");
        assert_eq!(
            received[0],
            (
                MessageType::CheckpointChangeLevel,
                MessagePayload::ChangeLevel(ChangeLevelPayload {
                    level_file: String::from("3.JN1"),
                    level_number: 3,
                }),
            )
        );
        assert!(
            checkpoint.should_remove(),
            "checkpoint should flag itself for removal after firing"
        );
    }

    /// Unit under test: [`CheckPointEntity::on_touch`] for a map-return
    /// destination.
    ///
    /// Preconditions: the checkpoint's string entry begins with the
    /// "load previous map" marker `!`; the recording handler is subscribed
    /// to [`MessageType::CheckpointChangeLevel`].
    ///
    /// Invariants asserted: the dispatched payload carries
    /// `level_number == MAP_LEVEL` (`-1`) and `level_file == "MAP.JN1"`.
    #[test]
    fn on_touch_dispatches_map_level_for_map_return_marker() {
        let cache = AssetCache::synthetic();
        let mut checkpoint =
            CheckPointEntity::new(&synthetic_checkpoint_object(0), Some("!"), &cache);
        let buf: RecordingBuffer = Arc::new(Mutex::new(Vec::new()));
        let mut dispatcher = MessageDispatcher::new();
        dispatcher.subscribe(
            MessageType::CheckpointChangeLevel,
            Box::new(RecordingHandler(Arc::clone(&buf))),
        );

        let state = RuntimeState::new();
        checkpoint.on_touch(&state, &mut dispatcher);

        let received = buf.lock().unwrap();
        assert_eq!(received.len(), 1, "exactly one transition dispatched");
        assert_eq!(
            received[0],
            (
                MessageType::CheckpointChangeLevel,
                MessagePayload::ChangeLevel(ChangeLevelPayload {
                    level_file: String::from("MAP.JN1"),
                    level_number: MAP_LEVEL,
                }),
            )
        );
    }
}
