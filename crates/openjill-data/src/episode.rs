//! Episode descriptor used by data utilities, the CLI, and the game runtime
//! to address per-episode file names and conventions without hardcoding
//! `JILL1`-specific literals.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

/// Static description of one Jill of the Jungle episode and the on-disk
/// filenames the original engine uses for it.
///
/// The string fields are the canonical uppercase names found inside the
/// original game data directory; case-insensitive lookups should still be used
/// when resolving them on disk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Episode {
    /// One-based episode number (1, 2, or 3 in retail Jill of the Jungle).
    pub number: u8,
    /// Episode-specific shape/tile/font/picture file name (e.g. `JILL1.SHA`).
    pub sha: &'static str,
    /// Episode-specific sound/text file name (e.g. `JILL1.VCL`).
    pub vcl: &'static str,
    /// Episode-specific configuration/high-score/save file name (e.g. `JILL1.CFG`).
    pub cfg: &'static str,
    /// File-name extension shared by all level/intro/map JN files for this
    /// episode, without the leading dot (e.g. `JN1`).
    ///
    /// Also used as the save-name prefix accepted by [`crate::cfg::CfgFile::from_bytes`].
    pub jn_ext: &'static str,
    /// Repository-relative default location of this episode's original data
    /// (e.g. `data/original/JILL1`).
    pub default_dir: &'static str,
    /// Repository-relative default root for debug-dump artifacts generated for
    /// this episode (e.g. `data/extracted/JILL1/debug`).
    pub default_dump_root: &'static str,
}

/// Episode 1 ("Jill of the Jungle").
pub const JILL1: Episode = Episode {
    number: 1,
    sha: "JILL1.SHA",
    vcl: "JILL1.VCL",
    cfg: "JILL1.CFG",
    jn_ext: "JN1",
    default_dir: "data/original/JILL1",
    default_dump_root: "data/extracted/JILL1/debug",
};

/// Episode 2 ("Jill Goes Underground").
pub const JILL2: Episode = Episode {
    number: 2,
    sha: "JILL2.SHA",
    vcl: "JILL2.VCL",
    cfg: "JILL2.CFG",
    jn_ext: "JN2",
    default_dir: "data/original/JILL2",
    default_dump_root: "data/extracted/JILL2/debug",
};

/// Episode 3 ("Jill Saves the Prince").
pub const JILL3: Episode = Episode {
    number: 3,
    sha: "JILL3.SHA",
    vcl: "JILL3.VCL",
    cfg: "JILL3.CFG",
    jn_ext: "JN3",
    default_dir: "data/original/JILL3",
    default_dump_root: "data/extracted/JILL3/debug",
};

/// Static table of all retail Jill of the Jungle episodes.
pub const ALL: &[Episode] = &[JILL1, JILL2, JILL3];

impl Episode {
    /// Returns the canonical descriptor for episode `number` (1, 2, or 3), or
    /// `None` when no retail episode matches.
    pub fn from_number(number: u8) -> Option<&'static Self> {
        ALL.iter().find(|episode| episode.number == number)
    }

    /// Returns the canonical descriptor whose [`Self::jn_ext`] equals
    /// `extension` case-insensitively, or `None` when no retail episode
    /// matches.
    ///
    /// `extension` may be supplied with or without a leading dot.
    pub fn from_jn_extension(extension: &str) -> Option<&'static Self> {
        let trimmed = extension.trim_start_matches('.');
        ALL.iter()
            .find(|episode| episode.jn_ext.eq_ignore_ascii_case(trimmed))
    }

    /// Scans `data_dir` for an episode-specific SHA file and returns the
    /// matching descriptor when exactly one is present.
    ///
    /// Returns `None` when zero, more than one, or no readable SHA files
    /// match. Callers should fall back to their own default in that case
    /// rather than treating ambiguity as a failure.
    pub fn detect_from_dir(data_dir: &Path) -> Option<&'static Self> {
        let entries = fs::read_dir(data_dir).ok()?;
        let mut matched: Option<&'static Self> = None;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            for episode in ALL {
                if name.eq_ignore_ascii_case(episode.sha) {
                    if matched.is_some() {
                        return None;
                    }
                    matched = Some(episode);
                }
            }
        }
        matched
    }

    /// Builds the intro JN file name for this episode (e.g. `INTRO.JN1`).
    pub fn intro_jn(&self) -> String {
        format!("INTRO.{}", self.jn_ext)
    }

    /// Builds the world-map JN file name for this episode (e.g. `MAP.JN1`).
    pub fn map_jn(&self) -> String {
        format!("MAP.{}", self.jn_ext)
    }

    /// Builds the level JN file name for `level_number` (e.g. `1.JN1`).
    pub fn level_jn(&self, level_number: i32) -> String {
        format!("{level_number}.{}", self.jn_ext)
    }

    /// Returns `true` when `file_name` has the JN extension associated with
    /// this episode (case-insensitive comparison).
    pub fn has_jn_extension(&self, file_name: &OsStr) -> bool {
        Path::new(file_name)
            .extension()
            .and_then(OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case(self.jn_ext))
    }
}

/// Discovers top-level JN files matching `episode`'s [`Episode::jn_ext`]
/// inside `data_dir`, returned in deterministic case-insensitive lexicographic
/// order.
///
/// Returns an empty vector when `data_dir` is not a directory; surfaces any
/// other read error to the caller.
pub fn discover_jn_files(data_dir: &Path, episode: &Episode) -> std::io::Result<Vec<String>> {
    if !data_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut file_names = fs::read_dir(data_dir)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let is_file = entry.file_type().ok()?.is_file();
            if !is_file {
                return None;
            }
            let file_name = entry.file_name();
            if episode.has_jn_extension(&file_name) {
                Some(file_name.to_string_lossy().into_owned())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    file_names.sort_by(|left, right| {
        left.to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right))
    });
    Ok(file_names)
}

#[cfg(test)]
mod tests {
    use super::{ALL, Episode, JILL1, JILL2, JILL3, discover_jn_files};
    use assert2::check;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unit under test: [`Episode::from_number`] lookup.
    ///
    /// Preconditions: callers pass the three retail episode numbers and one
    /// out-of-range number.
    ///
    /// Invariants asserted: known numbers resolve to the matching constant
    /// descriptor; unknown numbers return `None`.
    #[test]
    fn from_number_resolves_known_episodes_only() {
        check!(Episode::from_number(1) == Some(&JILL1));
        check!(Episode::from_number(2) == Some(&JILL2));
        check!(Episode::from_number(3) == Some(&JILL3));
        check!(Episode::from_number(0).is_none());
        check!(Episode::from_number(4).is_none());
    }

    /// Unit under test: [`Episode::from_jn_extension`] lookup.
    ///
    /// Preconditions: callers pass JN extensions with and without leading
    /// dots, in mixed case, and one unknown extension.
    ///
    /// Invariants asserted: known extensions resolve case-insensitively to
    /// the matching descriptor regardless of leading dot; unknown extensions
    /// return `None`.
    #[test]
    fn from_jn_extension_resolves_known_extensions_case_insensitively() {
        check!(Episode::from_jn_extension("JN1") == Some(&JILL1));
        check!(Episode::from_jn_extension(".jn2") == Some(&JILL2));
        check!(Episode::from_jn_extension("Jn3") == Some(&JILL3));
        check!(Episode::from_jn_extension("JN9").is_none());
    }

    /// Unit under test: [`Episode::intro_jn`], [`Episode::map_jn`], and
    /// [`Episode::level_jn`] filename helpers.
    ///
    /// Preconditions: descriptors for each retail episode are exercised.
    ///
    /// Invariants asserted: the helpers produce the canonical uppercase JN
    /// file names per episode, and `level_jn` formats the level number
    /// followed by the episode extension.
    #[test]
    fn filename_helpers_build_canonical_jn_names_per_episode() {
        check!(JILL1.intro_jn() == "INTRO.JN1");
        check!(JILL2.intro_jn() == "INTRO.JN2");
        check!(JILL3.intro_jn() == "INTRO.JN3");
        check!(JILL1.map_jn() == "MAP.JN1");
        check!(JILL2.map_jn() == "MAP.JN2");
        check!(JILL3.map_jn() == "MAP.JN3");
        check!(JILL1.level_jn(1) == "1.JN1");
        check!(JILL2.level_jn(42) == "42.JN2");
    }

    /// Unit under test: [`Episode::detect_from_dir`].
    ///
    /// Preconditions: a temporary directory contains exactly one episode
    /// SHA file in mixed case; an empty directory; a directory with two
    /// episode SHA files.
    ///
    /// Invariants asserted: a unique SHA match returns the matching episode,
    /// an absent SHA returns `None`, and multiple SHA matches return `None`
    /// so callers can fall back to a default deterministically.
    #[test]
    fn detect_from_dir_resolves_unique_sha_and_ignores_ambiguity() {
        let single = TempDirGuard::new("openjill-data-episode-single");
        fs::write(single.path().join("jill1.sha"), [0u8; 4]).expect("write jill1.sha");
        check!(Episode::detect_from_dir(single.path()) == Some(&JILL1));

        let empty = TempDirGuard::new("openjill-data-episode-empty");
        check!(Episode::detect_from_dir(empty.path()).is_none());

        let ambiguous = TempDirGuard::new("openjill-data-episode-ambiguous");
        fs::write(ambiguous.path().join("JILL1.SHA"), [0u8; 4]).expect("write jill1.sha");
        fs::write(ambiguous.path().join("JILL2.SHA"), [0u8; 4]).expect("write jill2.sha");
        check!(Episode::detect_from_dir(ambiguous.path()).is_none());
    }

    /// Unit under test: [`Episode::has_jn_extension`] case-insensitive matching.
    ///
    /// Preconditions: representative file names with and without JN-style
    /// extensions are checked against each retail episode descriptor.
    ///
    /// Invariants asserted: only file names whose extension matches the
    /// episode's [`Episode::jn_ext`] (case-insensitive) report `true`.
    #[test]
    fn has_jn_extension_matches_only_episode_specific_files() {
        check!(JILL1.has_jn_extension("LEVEL1.JN1".as_ref()));
        check!(JILL1.has_jn_extension("alpha.jn1".as_ref()));
        check!(!JILL1.has_jn_extension("LEVEL1.JN2".as_ref()));
        check!(!JILL1.has_jn_extension("README".as_ref()));
        check!(JILL2.has_jn_extension("BETA.Jn2".as_ref()));
    }

    /// Unit under test: the [`ALL`] table covers all retail episode descriptors.
    ///
    /// Preconditions: none.
    ///
    /// Invariants asserted: every retail episode constant appears in [`ALL`]
    /// exactly once and the numbers form the contiguous range 1..=3.
    #[test]
    fn all_table_lists_each_retail_episode_once() {
        let numbers: Vec<u8> = ALL.iter().map(|episode| episode.number).collect();
        check!(numbers == vec![1, 2, 3]);
    }

    /// Unit under test: [`discover_jn_files`] file enumeration.
    ///
    /// Preconditions: a temporary directory contains JN files for several
    /// episodes plus a non-JN sibling and a nested subdirectory.
    ///
    /// Invariants asserted: only files whose extension matches the supplied
    /// episode's [`Episode::jn_ext`] are returned, names are returned with
    /// their original capitalization, ordering is case-insensitive
    /// lexicographic, and non-file entries are ignored.
    #[test]
    fn discover_jn_files_filters_by_episode_extension_and_orders_deterministically() {
        let temp = TempDirGuard::new("openjill-data-discover");
        fs::write(temp.path().join("alpha.JN1"), [0u8; 4]).expect("write alpha.JN1");
        fs::write(temp.path().join("BETA.jn1"), [0u8; 4]).expect("write BETA.jn1");
        fs::write(temp.path().join("gamma.JN2"), [0u8; 4]).expect("write gamma.JN2");
        fs::write(temp.path().join("README"), [0u8; 4]).expect("write README");
        fs::create_dir_all(temp.path().join("nested.JN1")).expect("create nested.JN1 dir");

        let jn1 = discover_jn_files(temp.path(), &JILL1).expect("discover JN1");
        check!(jn1 == vec!["alpha.JN1".to_string(), "BETA.jn1".to_string()]);

        let jn2 = discover_jn_files(temp.path(), &JILL2).expect("discover JN2");
        check!(jn2 == vec!["gamma.JN2".to_string()]);
    }

    /// Unit under test: [`discover_jn_files`] missing-directory handling.
    ///
    /// Preconditions: callers pass a path that does not exist.
    ///
    /// Invariants asserted: discovery returns an empty list rather than an
    /// error, matching the prior CLI behaviour against a fresh checkout.
    #[test]
    fn discover_jn_files_returns_empty_when_directory_missing() {
        let missing = std::env::temp_dir().join("openjill-data-discover-missing-dir-xyz");
        let _ = fs::remove_dir_all(&missing);
        let files = discover_jn_files(&missing, &JILL1).expect("missing dir should be empty");
        check!(files.is_empty());
    }

    /// Owned temporary directory that removes itself on drop.
    struct TempDirGuard(PathBuf);

    impl TempDirGuard {
        /// Creates a temporary directory whose name combines `prefix` with a
        /// nanosecond timestamp.
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
            fs::create_dir_all(&path).expect("create temp directory");
            Self(path)
        }

        /// Returns the temporary directory path.
        fn path(&self) -> &std::path::Path {
            self.0.as_path()
        }
    }

    impl Drop for TempDirGuard {
        /// Best-effort recursive deletion of the temporary directory.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
