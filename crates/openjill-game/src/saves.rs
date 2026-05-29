//! Writable runtime storage for saves, high scores, and config (epic 7).
//!
//! The shipped/original data directory is read-only; everything the game
//! mutates at runtime (the `JILL1.CFG` copy, save-game snapshots) lives in a
//! separate per-user, per-episode writable directory resolved here.

use std::io;
use std::path::{Path, PathBuf};

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
    pub fn read(&self, file: &str) -> io::Result<Vec<u8>> {
        std::fs::read(self.root.join(file))
    }

    /// Returns `true` when `file` exists in the runtime directory.
    pub fn exists(&self, file: &str) -> bool {
        self.root.join(file).exists()
    }

    /// Atomically writes `bytes` to `file`: writes a sibling `.tmp` then renames
    /// over the target, creating the directory first.  A crash mid-write leaves
    /// either the old file or the complete new one, never a partial.
    pub fn write_atomic(&self, file: &str, bytes: &[u8]) -> io::Result<()> {
        std::fs::create_dir_all(&self.root)?;
        let target = self.root.join(file);
        let tmp = self.root.join(format!("{file}.tmp"));
        std::fs::write(&tmp, bytes)?;
        std::fs::rename(&tmp, &target)
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
}
