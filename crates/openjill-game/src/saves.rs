//! Writable runtime storage for saves, high scores, and config (epic 7).
//!
//! The shipped/original data directory is read-only; everything the game
//! mutates at runtime (the `JILL1.CFG` copy, save-game snapshots) lives in a
//! separate per-user, per-episode writable directory resolved here.

use std::ffi::OsStr;
use std::io;
use std::path::{Component, Path, PathBuf};

use openjill_data::cfg::{CfgFile, CfgHighScore, CfgReadError, CfgSaveSlot};
use openjill_data::{DataDirectory, Episode};
use thiserror::Error;

/// Environment variable that overrides the writable state base directory.
const STATE_DIR_ENV: &str = "OPENJILL_STATE_DIR";
/// Application subdirectory under the platform data dir.
const APP_DIR: &str = "openjill";

/// A per-episode, user-writable directory for runtime state (CFG, saves).
///
/// Resolution order for the base directory:
/// 1. `OPENJILL_STATE_DIR` (explicit override).
/// 2. `dirs::data_dir()/openjill` (platform per-user data dir).
/// 3. `std::env::temp_dir()/openjill` (last-resort fallback).
///
/// The per-episode directory is `{base}/{episode}`. The directory is created
/// lazily on the first write; resolution and construction never panic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeDir {
    root: PathBuf,
}

impl RuntimeDir {
    /// Resolves the writable directory for `episode` from the environment and
    /// platform data dir.
    pub fn for_episode(episode: &str) -> Self {
        let env_override = std::env::var_os(STATE_DIR_ENV).map(PathBuf::from);
        Self {
            root: resolve_root(env_override, dirs::data_dir(), episode),
        }
    }

    /// Builds a `RuntimeDir` rooted at an explicit path (tests, callers that
    /// already know the location).
    pub fn with_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Returns the per-episode directory path (may not exist yet).
    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Reads a file inside the runtime directory.
    ///
    /// `file` must be a simple relative filename (no separators, `..`, or
    /// absolute paths) so reads cannot escape the runtime directory.
    pub fn read(&self, file: &str) -> io::Result<Vec<u8>> {
        validate_filename(file)?;
        std::fs::read(self.root.join(file))
    }

    /// Returns `true` when `file` (a simple relative filename) exists in the
    /// runtime directory.  An unsafe filename returns `false`.
    pub fn exists(&self, file: &str) -> bool {
        validate_filename(file).is_ok() && self.root.join(file).exists()
    }

    /// Atomically writes `bytes` to `file`: writes a sibling `.tmp` then renames
    /// over the target, creating the directory first.  A crash mid-write leaves
    /// either the old file or the complete new one, never a partial.
    ///
    /// `file` must be a simple relative filename so writes stay inside the
    /// runtime directory.
    pub fn write_atomic(&self, file: &str, bytes: &[u8]) -> io::Result<()> {
        validate_filename(file)?;
        std::fs::create_dir_all(&self.root)?;
        let target = self.root.join(file);
        let tmp = self.root.join(format!("{file}.tmp"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &target)
    }
}

/// Rejects anything other than a single, normal relative filename so a caller
/// cannot read or write outside the runtime directory (no separators, `..`,
/// root, prefix, or absolute paths).  Mirrors the containment checks
/// `DataDirectory` applies to its read paths.
fn validate_filename(file: &str) -> io::Result<()> {
    let mut components = Path::new(file).components();
    let is_single_normal = matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(segment)), None) if segment == OsStr::new(file)
    );
    if is_single_normal {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("runtime-dir filename must be a simple relative name: {file:?}"),
        ))
    }
}

/// Computes the per-episode runtime directory from the resolved inputs.
///
/// Split out from [`RuntimeDir::for_episode`] so the precedence is unit-testable
/// without touching process-global environment state.
fn resolve_root(
    env_override: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    episode: &str,
) -> PathBuf {
    let base = env_override
        .or_else(|| data_dir.map(|dir| dir.join(APP_DIR)))
        .unwrap_or_else(|| std::env::temp_dir().join(APP_DIR));
    base.join(episode)
}

/// Errors raised by [`SaveStore`] operations.
#[derive(Debug, Error)]
pub enum SaveStoreError {
    /// A filesystem read/write of the runtime directory failed.
    #[error("save-store I/O error: {0}")]
    Io(#[from] io::Error),
    /// The config bytes could not be parsed.
    #[error("config parse error: {0}")]
    Cfg(#[from] CfgReadError),
    /// A save-slot index outside the episode's save table was requested.
    #[error("save slot {slot} out of range (0..{count})")]
    InvalidSlot {
        /// The requested slot index.
        slot: usize,
        /// Number of save slots the episode provides.
        count: usize,
    },
}

/// Runtime saves, high scores, and config for one episode.
///
/// Owns the writable [`RuntimeDir`] and the working [`CfgFile`].  The CFG is
/// loaded from the runtime directory if present, otherwise seeded from the
/// original (read-only) `JILL1.CFG` when available, otherwise from defaults;
/// the seed is persisted best-effort.  All mutations write through the runtime
/// directory and never touch the original data.
pub struct SaveStore {
    runtime: RuntimeDir,
    cfg: CfgFile,
    /// Config file name (e.g. `JILL1.CFG`).
    cfg_file: &'static str,
    /// Save-file / config name prefix (e.g. `JN1`).
    prefix: &'static str,
}

impl SaveStore {
    /// Opens the store for `episode`, resolving the runtime directory from the
    /// environment / platform data dir.
    pub fn open(original: &DataDirectory, episode: &Episode) -> Result<Self, SaveStoreError> {
        Self::open_with_runtime(RuntimeDir::for_episode(episode.jn_ext), original, episode)
    }

    /// Opens the store using an explicit runtime directory (tests, callers that
    /// already resolved the location).
    pub fn open_with_runtime(
        runtime: RuntimeDir,
        original: &DataDirectory,
        episode: &Episode,
    ) -> Result<Self, SaveStoreError> {
        let cfg = if runtime.exists(episode.cfg) {
            // Existing runtime config is authoritative; a parse failure here is
            // surfaced (corrupt local state) rather than silently discarded.
            CfgFile::from_bytes(runtime.read(episode.cfg)?, episode.jn_ext)?
        } else {
            // Seed: copy the original config when present, else defaults.
            let cfg = match original
                .open_reader(episode.cfg)
                .ok()
                .map(|reader| reader.as_bytes().to_vec())
            {
                Some(bytes) => CfgFile::from_bytes(bytes, episode.jn_ext)?,
                None => CfgFile::empty(episode.jn_ext),
            };
            // Persist the seed so the next run reads it; ignore write failures
            // so a non-writable location degrades to in-memory rather than
            // crashing.
            let _ = runtime.write_atomic(episode.cfg, &cfg.to_bytes());
            cfg
        };
        Ok(Self {
            runtime,
            cfg,
            cfg_file: episode.cfg,
            prefix: episode.jn_ext,
        })
    }

    /// Returns the working runtime config (the live high-score and save-slot
    /// tables), so the start menu can render the runtime state rather than the
    /// original shipped `JILL1.CFG`.
    pub fn cfg(&self) -> &CfgFile {
        &self.cfg
    }

    /// Returns the current high-score table.
    pub fn high_scores(&self) -> &[CfgHighScore] {
        self.cfg.high_scores()
    }

    /// Returns the current save slots.
    pub fn save_slots(&self) -> &[CfgSaveSlot] {
        self.cfg.save_slots()
    }

    /// Writes a save-game snapshot to `slot`: the map JN bytes to
    /// `{prefix}SAVEM.{slot}` and the level JN bytes to `{prefix}SAVE.{slot}`
    /// (matching the original layout), names the slot, and persists the config.
    pub fn save_game(
        &mut self,
        slot: usize,
        name: &str,
        map_bytes: &[u8],
        level_bytes: &[u8],
    ) -> Result<(), SaveStoreError> {
        self.check_slot(slot)?;
        self.runtime
            .write_atomic(&format!("{}SAVEM.{slot}", self.prefix), map_bytes)?;
        self.runtime
            .write_atomic(&format!("{}SAVE.{slot}", self.prefix), level_bytes)?;
        self.cfg.set_save_slot_name(slot, name);
        self.persist_cfg()
    }

    /// Reads back a save-game snapshot from `slot` as `(map_bytes, level_bytes)`.
    pub fn load_game(&self, slot: usize) -> Result<(Vec<u8>, Vec<u8>), SaveStoreError> {
        self.check_slot(slot)?;
        let map = self.runtime.read(&format!("{}SAVEM.{slot}", self.prefix))?;
        let level = self.runtime.read(&format!("{}SAVE.{slot}", self.prefix))?;
        Ok((map, level))
    }

    /// Records a new high score (sorted, capped) and persists the config.
    pub fn record_high_score(&mut self, name: &str, score: i32) -> Result<(), SaveStoreError> {
        self.cfg.add_high_score(name, score);
        self.persist_cfg()
    }

    /// Validates a slot index against the episode's save table.
    fn check_slot(&self, slot: usize) -> Result<(), SaveStoreError> {
        let count = self.cfg.save_slots().len();
        if slot < count {
            Ok(())
        } else {
            Err(SaveStoreError::InvalidSlot { slot, count })
        }
    }

    /// Writes the working config back to the runtime directory.
    fn persist_cfg(&self) -> Result<(), SaveStoreError> {
        self.runtime
            .write_atomic(self.cfg_file, &self.cfg.to_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unique temp directory for a write/read round-trip test, without pulling
    /// in a tempdir dependency.
    fn unique_temp_root() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("openjill-test-{}-{nanos}", std::process::id()))
    }

    /// Unit under test: [`resolve_root`] precedence.
    #[test]
    fn resolve_root_prefers_env_then_data_dir_then_temp() {
        // 1. Env override wins and is joined with the episode.
        let with_env = resolve_root(
            Some(PathBuf::from("/state")),
            Some(PathBuf::from("/data")),
            "JILL1",
        );
        assert_eq!(with_env, PathBuf::from("/state/JILL1"));

        // 2. No env: platform data dir + app subdir + episode.
        let with_data = resolve_root(None, Some(PathBuf::from("/data")), "JILL1");
        assert_eq!(with_data, PathBuf::from("/data/openjill/JILL1"));

        // 3. Neither: temp dir fallback.
        let with_temp = resolve_root(None, None, "JILL1");
        assert_eq!(
            with_temp,
            std::env::temp_dir().join("openjill").join("JILL1")
        );
    }

    /// Unit under test: [`RuntimeDir::write_atomic`] + [`RuntimeDir::read`].
    ///
    /// Invariants asserted: a write creates the directory and the file, a read
    /// returns the bytes back, `exists` reflects presence, and no `.tmp` file
    /// is left behind.
    #[test]
    fn write_atomic_then_read_round_trips() {
        let root = unique_temp_root();
        let dir = RuntimeDir::with_root(&root);
        assert!(!dir.exists("JILL1.CFG"));

        dir.write_atomic("JILL1.CFG", b"hello").expect("write");
        assert!(dir.exists("JILL1.CFG"));
        assert_eq!(dir.read("JILL1.CFG").expect("read"), b"hello");
        assert!(!root.join("JILL1.CFG.tmp").exists(), "temp file cleaned up");

        // Overwrite is atomic and replaces the previous contents.
        dir.write_atomic("JILL1.CFG", b"world").expect("rewrite");
        assert_eq!(dir.read("JILL1.CFG").expect("reread"), b"world");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Reading a missing file surfaces a `NotFound` error rather than panicking.
    #[test]
    fn read_missing_file_errors() {
        let dir = RuntimeDir::with_root(unique_temp_root());
        let err = dir.read("absent.cfg").expect_err("missing file errors");
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// Unsafe filenames (separators, `..`, absolute) are rejected so reads and
    /// writes cannot escape the runtime directory.
    #[test]
    fn rejects_unsafe_filenames() {
        let dir = RuntimeDir::with_root(unique_temp_root());
        for bad in ["../escape", "a/b", "/etc/passwd", "..", ".", ""] {
            assert_eq!(
                dir.read(bad).unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "read({bad:?}) must be rejected"
            );
            assert_eq!(
                dir.write_atomic(bad, b"x").unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "write_atomic({bad:?}) must be rejected"
            );
            assert!(!dir.exists(bad), "exists({bad:?}) must be false");
        }
    }

    /// Save/load reject a slot index outside the episode's save table.
    #[test]
    fn save_load_reject_out_of_range_slot() {
        use openjill_data::episode::JILL1;

        let runtime_root = unique_temp_root();
        let mut store = SaveStore::open_with_runtime(
            RuntimeDir::with_root(&runtime_root),
            &DataDirectory::new(unique_temp_root()),
            &JILL1,
        )
        .expect("open seeds defaults");
        let out = store.save_slots().len();

        assert!(matches!(
            store.save_game(out, "X", b"m", b"l"),
            Err(SaveStoreError::InvalidSlot { .. })
        ));
        assert!(matches!(
            store.load_game(out),
            Err(SaveStoreError::InvalidSlot { .. })
        ));

        std::fs::remove_dir_all(&runtime_root).ok();
    }

    /// Unit under test: [`SaveStore`] save-game + high-score persistence across
    /// a reopen (simulating an app restart), with no original config (defaults).
    #[test]
    fn save_store_persists_across_reopen() {
        use openjill_data::episode::JILL1;

        let runtime_root = unique_temp_root();
        let original = DataDirectory::new(unique_temp_root()); // empty -> defaults

        {
            let mut store = SaveStore::open_with_runtime(
                RuntimeDir::with_root(&runtime_root),
                &original,
                &JILL1,
            )
            .expect("open seeds defaults");
            store.save_game(0, "SLOT0", b"MAP", b"LEVEL").expect("save");
            store
                .record_high_score("ACE", 1234)
                .expect("record high score");
        }

        let store =
            SaveStore::open_with_runtime(RuntimeDir::with_root(&runtime_root), &original, &JILL1)
                .expect("reopen reads persisted state");
        assert_eq!(store.save_slots()[0].name(), "SLOT0");
        assert_eq!(store.high_scores()[0].name(), "ACE");
        assert_eq!(store.high_scores()[0].score(), 1234);
        assert_eq!(
            store.load_game(0).expect("load"),
            (b"MAP".to_vec(), b"LEVEL".to_vec())
        );

        std::fs::remove_dir_all(&runtime_root).ok();
    }

    /// Unit under test: [`SaveStore::open`] seeding from an existing original
    /// `JILL1.CFG` and persisting the seed to the runtime directory.
    #[test]
    fn save_store_seeds_from_original_cfg() {
        use openjill_data::episode::JILL1;

        let original_root = unique_temp_root();
        std::fs::create_dir_all(&original_root).unwrap();
        let mut seed = CfgFile::empty("JN1");
        seed.add_high_score("ORIG", 777);
        std::fs::write(original_root.join("JILL1.CFG"), seed.to_bytes()).unwrap();

        let runtime_root = unique_temp_root();
        let store = SaveStore::open_with_runtime(
            RuntimeDir::with_root(&runtime_root),
            &DataDirectory::new(&original_root),
            &JILL1,
        )
        .expect("open seeds from original");

        assert_eq!(store.high_scores()[0].name(), "ORIG");
        assert_eq!(store.high_scores()[0].score(), 777);
        assert!(
            RuntimeDir::with_root(&runtime_root).exists("JILL1.CFG"),
            "seed persisted to runtime dir"
        );

        std::fs::remove_dir_all(&original_root).ok();
        std::fs::remove_dir_all(&runtime_root).ok();
    }
}
