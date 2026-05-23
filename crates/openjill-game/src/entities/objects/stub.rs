//! Catch-all object entity returned by the factory for unregistered JN object
//! types.
//!
//! Mirrors the "missing manager" branch from the Java reference: rather than
//! crashing the level loop when an unfamiliar `type` byte appears in a JN
//! file, the factory installs a stub that emits a single warning to stderr the
//! first time each unique type id is seen and is otherwise inert.  The stub
//! never draws, never moves, never reacts to touches or hits, and reports a
//! zero-sized bounding box at the object's spawn position so it cannot collide
//! with the player.

use std::collections::HashSet;
use std::sync::Mutex;

use openjill_core::{
    ActiveInput, BackgroundGrid, DeathKind, MessageDispatcher, ObjectEntity, Rect, RenderCommand,
    RuntimeState,
};
use openjill_data::jn::JnObject;

/// Set of object type ids that have already produced a warning, used to
/// suppress repeat log output when the same unregistered type appears in
/// multiple JN object records.
static WARNED_TYPES: Mutex<Option<HashSet<u8>>> = Mutex::new(None);

/// Stub object entity returned for unregistered JN object type ids.
pub struct StubObjectEntity {
    /// The raw object type byte from the JN file.
    type_id: u8,
    /// Spawn position in world pixels copied from the JN object record.
    x: i32,
    /// Spawn position in world pixels copied from the JN object record.
    y: i32,
}

impl StubObjectEntity {
    /// Builds a stub entity for the supplied `type_id` at the JN object's
    /// spawn position.
    ///
    /// Emits one `eprintln!` warning the first time each unique type id is
    /// seen so repeated stubs do not flood the log.
    pub fn new(type_id: u8, item: &JnObject) -> Self {
        if Self::record_warning(type_id) {
            eprintln!(
                "openjill-game: object type {type_id} has no registered entity; \
                 using stub (will not draw, update, or react to touches)"
            );
        }
        Self {
            type_id,
            x: i32::from(item.x()),
            y: i32::from(item.y()),
        }
    }

    /// Records that `type_id` has produced a warning.
    ///
    /// Returns `true` when this call inserted a new entry (the warning should
    /// be emitted) and `false` when the type id was already recorded.  The
    /// mutex is initialised lazily on first call so the static state can live
    /// in a `const`-friendly `Mutex::new(None)`.
    fn record_warning(type_id: u8) -> bool {
        let mut guard = WARNED_TYPES.lock().expect("WARNED_TYPES mutex poisoned");
        guard.get_or_insert_with(HashSet::new).insert(type_id)
    }

    /// Returns the object type id this stub was built for.
    ///
    /// Exposed for tests that verify the factory returns a stub for unknown
    /// types.
    pub fn type_id(&self) -> u8 {
        self.type_id
    }
}

impl ObjectEntity for StubObjectEntity {
    /// No-op: a stub never advances its state.
    fn update(
        &mut self,
        _input: &ActiveInput,
        _state: &RuntimeState,
        _backgrounds: &BackgroundGrid,
        _dispatcher: &mut MessageDispatcher,
    ) {
    }

    /// Returns `None` so a stub never contributes a render command.
    fn draw(&self) -> Option<RenderCommand> {
        None
    }

    /// No-op: a stub never reacts to a player touch.
    fn on_touch(&mut self, _state: &RuntimeState, _dispatcher: &mut MessageDispatcher) {}

    /// No-op: a stub never reacts to weapon hits.
    fn on_kill(&mut self, _damage: i32, _death_kind: DeathKind) {}

    /// Returns a zero-sized bounding box at the spawn position so the stub
    /// cannot register a collision with the player.
    fn bounding_box(&self) -> Rect {
        Rect::new(self.x, self.y, 0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::{StubObjectEntity, WARNED_TYPES};
    use openjill_core::{ObjectEntity, Rect};
    use openjill_data::jn::JnFile;

    /// Builds a JN file with one zero-initialised object record and returns the
    /// inner `JnObject` clone so tests can hand a real record to the entity.
    fn synthetic_object() -> openjill_data::jn::JnObject {
        const OBJECT_RECORD_BYTES: usize = 31;
        let object_count = 1usize;
        let total = 128 * 64 * 2 + 2 + object_count * OBJECT_RECORD_BYTES + 70;
        let mut bytes = vec![0u8; total];
        let count_off = 128 * 64 * 2;
        bytes[count_off..count_off + 2].copy_from_slice(&(object_count as u16).to_le_bytes());
        let jn = JnFile::from_bytes(bytes).expect("synthetic JN should parse");
        jn.objects()[0].clone()
    }

    /// Unit under test: `StubObjectEntity::new` records each unique type id
    /// exactly once.
    ///
    /// Preconditions: the `WARNED_TYPES` set is reset before the assertion so
    /// other tests in the binary do not leak state into this run.
    ///
    /// Invariants asserted: the first call for a given type id inserts (the
    /// helper returns `true`), and the second call for the same id returns
    /// `false`, mirroring the "warn once" guarantee.
    #[test]
    fn record_warning_suppresses_duplicates() {
        {
            let mut guard = WARNED_TYPES.lock().unwrap();
            *guard = None;
        }
        assert!(StubObjectEntity::record_warning(99));
        assert!(!StubObjectEntity::record_warning(99));
        assert!(StubObjectEntity::record_warning(100));
    }

    /// Unit under test: `StubObjectEntity` returns a zero-sized bounding box.
    #[test]
    fn stub_bounding_box_is_zero_sized() {
        let stub = StubObjectEntity::new(123, &synthetic_object());
        let bbox = stub.bounding_box();
        assert_eq!(bbox, Rect::new(0, 0, 0, 0));
    }
}
