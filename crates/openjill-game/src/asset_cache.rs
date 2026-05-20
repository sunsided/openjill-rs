//! Asset cache that holds pre-loaded episode game data files.

use openjill_data::cfg::{CfgFile, CfgReadError};
use openjill_data::dma::{DmaFile, DmaReadError};
use openjill_data::sha::{ShaFile, ShaReadError};
use openjill_data::vcl::{VclFile, VclReadError};
use openjill_data::{DataDirectory, DataDirectoryError};
use thiserror::Error;

/// Pre-loaded and cached game data files for one episode.
///
/// Constructed once at application start via [`AssetCache::load`]. Individual
/// screens borrow from the cache; they do not own or re-load it. JN files are
/// loaded per screen transition because each screen uses a different JN file.
#[derive(Debug)]
pub struct AssetCache {
    /// Parsed `JILL.DMA` background map-code to tileset/tile/flags lookup.
    pub dma: DmaFile,
    /// Parsed `JILL1.SHA` tileset, tile, font, and picture data.
    pub sha: ShaFile,
    /// Parsed `JILL1.VCL` text entries.
    pub vcl: VclFile,
    /// Parsed `JILL1.CFG` high scores and save slots.
    pub cfg: CfgFile,
}

impl AssetCache {
    /// Load all required assets from `data_dir`.
    ///
    /// Opens each required file via case-insensitive lookup and fails fast with
    /// a descriptive error if any file is missing or fails to parse.
    pub fn load(data_dir: &DataDirectory) -> Result<Self, AssetError> {
        let dma = {
            let mut reader =
                data_dir
                    .open_reader("JILL.DMA")
                    .map_err(|source| AssetError::Open {
                        filename: "JILL.DMA",
                        source,
                    })?;
            DmaFile::parse(&mut reader).map_err(|source| AssetError::Dma { source })?
        };
        let sha = {
            let mut reader =
                data_dir
                    .open_reader("JILL1.SHA")
                    .map_err(|source| AssetError::Open {
                        filename: "JILL1.SHA",
                        source,
                    })?;
            ShaFile::parse(&mut reader).map_err(|source| AssetError::Sha { source })?
        };
        let vcl = {
            let mut reader =
                data_dir
                    .open_reader("JILL1.VCL")
                    .map_err(|source| AssetError::Open {
                        filename: "JILL1.VCL",
                        source,
                    })?;
            VclFile::parse(&mut reader).map_err(|source| AssetError::Vcl { source })?
        };
        let cfg = {
            let mut reader =
                data_dir
                    .open_reader("JILL1.CFG")
                    .map_err(|source| AssetError::Open {
                        filename: "JILL1.CFG",
                        source,
                    })?;
            CfgFile::parse(&mut reader, "JN1").map_err(|source| AssetError::Cfg { source })?
        };
        Ok(Self { dma, sha, vcl, cfg })
    }
}

/// Error returned when loading required assets from a `DataDirectory` fails.
#[derive(Debug, Error)]
pub enum AssetError {
    /// A required file could not be opened from the data directory.
    #[error("failed to open {filename}: {source}")]
    Open {
        /// Name of the file that could not be opened.
        filename: &'static str,
        /// Underlying directory or I/O error.
        #[source]
        source: DataDirectoryError,
    },
    /// `JILL.DMA` parse failed.
    #[error("failed to parse JILL.DMA: {source}")]
    Dma {
        /// Underlying parse error.
        #[source]
        source: DmaReadError,
    },
    /// `JILL1.SHA` parse failed.
    #[error("failed to parse JILL1.SHA: {source}")]
    Sha {
        /// Underlying parse error.
        #[source]
        source: ShaReadError,
    },
    /// `JILL1.VCL` parse failed.
    #[error("failed to parse JILL1.VCL: {source}")]
    Vcl {
        /// Underlying parse error.
        #[source]
        source: VclReadError,
    },
    /// `JILL1.CFG` parse failed.
    #[error("failed to parse JILL1.CFG: {source}")]
    Cfg {
        /// Underlying parse error.
        #[source]
        source: CfgReadError,
    },
}

#[cfg(test)]
mod tests {
    use super::{AssetCache, AssetError};
    use openjill_data::DataDirectory;

    /// Unit under test: `AssetCache::load`.
    ///
    /// Preconditions: a temporary directory is used that exists on disk but
    /// contains none of the required game files.
    ///
    /// Invariants asserted: `load` returns an `AssetError::Open` error naming
    /// `"JILL.DMA"` as the first missing file, confirming fail-fast behaviour.
    #[test]
    fn load_returns_error_when_required_file_absent() {
        let dir = DataDirectory::new(std::env::temp_dir());
        match AssetCache::load(&dir) {
            Err(AssetError::Open { filename, .. }) => {
                assert_eq!(filename, "JILL.DMA");
            }
            other => panic!("expected AssetError::Open for JILL.DMA, got {other:?}"),
        }
    }
}
