//! Asset cache that holds pre-loaded episode game data files.

use openjill_data::cfg::{CfgFile, CfgReadError};
use openjill_data::dma::{DmaFile, DmaReadError};
use openjill_data::episode::Episode;
use openjill_data::jn::{JnFile, JnReadError};
use openjill_data::sha::{ShaFile, ShaReadError};
use openjill_data::vcl::{VclFile, VclReadError};
use openjill_data::{DataDirectory, DataDirectoryError};
use thiserror::Error;

/// Shared DMA file name used by every episode for tile metadata.
const SHARED_DMA_FILE: &str = "JILL.DMA";

/// Pre-loaded and cached game data files for one episode.
///
/// Constructed once at application start via [`AssetCache::load`]. Individual
/// screens clone the fields they need at construction time; they do not
/// re-load from disk. The episode's intro JN file ([`Episode::intro_jn`]) is
/// loaded here (rather than per-screen) because it is shared across the
/// start-menu and all INTRO-backed special screens and must be available
/// immediately when the application boots.
#[derive(Debug)]
pub struct AssetCache {
    /// Parsed `JILL.DMA` background map-code to tileset/tile/flags lookup.
    pub dma: DmaFile,
    /// Parsed episode SHA file ([`Episode::sha`]) tileset, tile, font, and picture data.
    pub sha: ShaFile,
    /// Parsed episode VCL file ([`Episode::vcl`]) text entries.
    pub vcl: VclFile,
    /// Parsed episode CFG file ([`Episode::cfg`]) high scores and save slots.
    pub cfg: CfgFile,
    /// Parsed intro JN file ([`Episode::intro_jn`]) background and object
    /// layers shared across all INTRO-backed screens (start menu, story,
    /// credits, ordering info, and noisemaker).
    pub intro_jn: JnFile,
}

impl AssetCache {
    /// Load all required assets for `episode` from `data_dir`.
    ///
    /// Opens each required file via case-insensitive lookup and fails fast with
    /// a descriptive error if any file is missing or fails to parse.
    pub fn load(data_dir: &DataDirectory, episode: &Episode) -> Result<Self, AssetError> {
        let dma = load_dma(data_dir)?;
        let sha = load_sha(data_dir, episode.sha)?;
        let vcl = load_vcl(data_dir, episode.vcl)?;
        let cfg = load_cfg(data_dir, episode.cfg, episode.jn_ext)?;
        let intro_jn_file = episode.intro_jn();
        let intro_jn = load_intro_jn(data_dir, &intro_jn_file)?;
        Ok(Self {
            dma,
            sha,
            vcl,
            cfg,
            intro_jn,
        })
    }
}

/// Opens and parses the shared `JILL.DMA` file.
fn load_dma(data_dir: &DataDirectory) -> Result<DmaFile, AssetError> {
    let mut reader = data_dir
        .open_reader(SHARED_DMA_FILE)
        .map_err(|source| AssetError::Open {
            filename: SHARED_DMA_FILE.into(),
            source: Box::new(source),
        })?;
    DmaFile::parse(&mut reader).map_err(|source| AssetError::Dma {
        source: Box::new(source),
    })
}

/// Opens and parses the episode's SHA file.
fn load_sha(data_dir: &DataDirectory, filename: &str) -> Result<ShaFile, AssetError> {
    let mut reader = data_dir
        .open_reader(filename)
        .map_err(|source| AssetError::Open {
            filename: filename.into(),
            source: Box::new(source),
        })?;
    ShaFile::parse(&mut reader).map_err(|source| AssetError::Sha {
        filename: filename.into(),
        source: Box::new(source),
    })
}

/// Opens and parses the episode's VCL file.
fn load_vcl(data_dir: &DataDirectory, filename: &str) -> Result<VclFile, AssetError> {
    let mut reader = data_dir
        .open_reader(filename)
        .map_err(|source| AssetError::Open {
            filename: filename.into(),
            source: Box::new(source),
        })?;
    VclFile::parse(&mut reader).map_err(|source| AssetError::Vcl {
        filename: filename.into(),
        source: Box::new(source),
    })
}

/// Opens and parses the episode's CFG file with the episode-specific save prefix.
fn load_cfg(
    data_dir: &DataDirectory,
    filename: &str,
    save_prefix: &str,
) -> Result<CfgFile, AssetError> {
    let mut reader = data_dir
        .open_reader(filename)
        .map_err(|source| AssetError::Open {
            filename: filename.into(),
            source: Box::new(source),
        })?;
    CfgFile::parse(&mut reader, save_prefix).map_err(|source| AssetError::Cfg {
        filename: filename.into(),
        source: Box::new(source),
    })
}

/// Opens and parses the episode's intro JN file.
fn load_intro_jn(data_dir: &DataDirectory, filename: &str) -> Result<JnFile, AssetError> {
    let mut reader = data_dir
        .open_reader(filename)
        .map_err(|source| AssetError::Open {
            filename: filename.into(),
            source: Box::new(source),
        })?;
    JnFile::parse(&mut reader).map_err(|source| AssetError::IntroJn {
        filename: filename.into(),
        source: Box::new(source),
    })
}

/// Error returned when loading required assets from a `DataDirectory` fails.
///
/// Source errors are boxed and filenames stored as `Box<str>` so the variants
/// stay compact enough to avoid `clippy::result_large_err` warnings even
/// though episode-specific filenames cannot be `&'static str`.
#[derive(Debug, Error)]
pub enum AssetError {
    /// A required file could not be opened from the data directory.
    #[error("failed to open {filename}: {source}")]
    Open {
        /// Name of the file that could not be opened.
        filename: Box<str>,
        /// Underlying directory or I/O error.
        #[source]
        source: Box<DataDirectoryError>,
    },
    /// `JILL.DMA` parse failed.
    #[error("failed to parse {SHARED_DMA_FILE}: {source}")]
    Dma {
        /// Underlying parse error.
        #[source]
        source: Box<DmaReadError>,
    },
    /// Episode SHA parse failed.
    #[error("failed to parse {filename}: {source}")]
    Sha {
        /// Episode SHA file that failed to parse.
        filename: Box<str>,
        /// Underlying parse error.
        #[source]
        source: Box<ShaReadError>,
    },
    /// Episode VCL parse failed.
    #[error("failed to parse {filename}: {source}")]
    Vcl {
        /// Episode VCL file that failed to parse.
        filename: Box<str>,
        /// Underlying parse error.
        #[source]
        source: Box<VclReadError>,
    },
    /// Episode CFG parse failed.
    #[error("failed to parse {filename}: {source}")]
    Cfg {
        /// Episode CFG file that failed to parse.
        filename: Box<str>,
        /// Underlying parse error.
        #[source]
        source: Box<CfgReadError>,
    },
    /// Intro JN parse failed.
    #[error("failed to parse {filename}: {source}")]
    IntroJn {
        /// Intro JN file that failed to parse.
        filename: Box<str>,
        /// Underlying parse error.
        #[source]
        source: Box<JnReadError>,
    },
}

#[cfg(test)]
impl AssetCache {
    /// Constructs a minimal valid asset cache from zero-byte synthetic
    /// buffers, used by tests that exercise screens and entities without
    /// loading real game files.
    pub(crate) fn synthetic() -> Self {
        use openjill_data::episode;
        const SHA_HEADER_BYTES: usize = 128 * 4 + 128 * 2;
        const VCL_MIN_BYTES: usize = 400 + 40 * 4 + 40 * 2;
        const CFG_MIN_BYTES: usize = 10 * 10 + 20 + 10 * 4 + 6 * 12 + 2 + 2 + 6 * 2 + 2 + 2 + 2;
        const JN_MIN_BYTES: usize = 128 * 64 * 2 + 2 + 70;
        Self {
            dma: DmaFile::from_bytes(vec![]).expect("empty DMA should parse"),
            sha: ShaFile::from_bytes(vec![0u8; SHA_HEADER_BYTES])
                .expect("zero SHA header should parse"),
            vcl: VclFile::from_bytes(vec![0u8; VCL_MIN_BYTES]).expect("zero VCL should parse"),
            cfg: CfgFile::from_bytes(vec![0u8; CFG_MIN_BYTES], episode::JILL1.jn_ext)
                .expect("zero CFG should parse"),
            intro_jn: JnFile::from_bytes(vec![0u8; JN_MIN_BYTES]).expect("zero JN should parse"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AssetCache, AssetError, SHARED_DMA_FILE};
    use openjill_data::DataDirectory;
    use openjill_data::episode;

    /// Unit under test: `AssetCache::load`.
    ///
    /// Preconditions: a temporary directory is used that exists on disk but
    /// contains none of the required game files.
    ///
    /// Invariants asserted: `load` returns an `AssetError::Open` error naming
    /// the shared DMA file as the first missing file, confirming fail-fast
    /// behaviour regardless of the supplied episode.
    #[test]
    fn load_returns_error_when_required_file_absent() {
        let dir = DataDirectory::new(std::env::temp_dir());
        match AssetCache::load(&dir, &episode::JILL1) {
            Err(AssetError::Open { filename, .. }) => {
                assert_eq!(&*filename, SHARED_DMA_FILE);
            }
            other => panic!("expected AssetError::Open for {SHARED_DMA_FILE}, got {other:?}"),
        }
    }
}
