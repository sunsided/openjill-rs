//! Data-layer crate for OpenJill: byte readers, parsers, and on-disk
//! data-directory helpers used by the rest of the port.

#![forbid(unsafe_code)]

/// Parser for `JILL.DMA` tile-metadata files.
pub mod dma;
/// Parser for the text-entry portion of `JILL1.VCL`.
pub mod vcl;

use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Failure modes raised by [`ByteReader`] while consuming bytes.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ByteReaderError {
    /// The reader hit end-of-file before the requested bytes were available.
    UnexpectedEof {
        /// Human-readable description of the read attempted at the failure site.
        operation: &'static str,
        /// Offset at which the failing read started.
        offset: usize,
        /// Number of bytes the read attempted to consume.
        requested: usize,
        /// Total length of the underlying buffer.
        len: usize,
    },
    /// A `seek` request targeted an offset past the end of the buffer.
    InvalidSeek {
        /// Offset that was requested.
        requested: usize,
        /// Total length of the underlying buffer.
        len: usize,
    },
    /// A `skip` request would have caused the offset arithmetic to overflow.
    OffsetOverflow {
        /// Current offset before the failing skip.
        offset: usize,
        /// Number of bytes the caller asked to skip.
        delta: usize,
    },
}

impl Display for ByteReaderError {
    /// Formats the error in a way that includes operation, offset, and buffer length.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof {
                operation,
                offset,
                requested,
                len,
            } => write!(
                f,
                "cannot {operation}: offset {offset}, need {requested} bytes, file length {len}"
            ),
            Self::InvalidSeek { requested, len } => {
                write!(f, "invalid seek to {requested}: file length is {len}")
            }
            Self::OffsetOverflow { offset, delta } => write!(
                f,
                "cannot skip {delta} bytes from offset {offset}: offset overflow"
            ),
        }
    }
}

impl Error for ByteReaderError {}

/// Failure modes raised when resolving paths inside a [`DataDirectory`].
#[derive(Debug)]
pub enum DataDirectoryError {
    /// An underlying I/O operation failed (e.g. reading a directory entry).
    Io(std::io::Error),
    /// The path supplied by the caller is not a valid relative path inside
    /// the data directory (absolute paths, parent components, prefixes, or
    /// roots are rejected).
    InvalidRelativePath {
        /// The original path requested by the caller.
        requested: PathBuf,
    },
    /// Case-insensitive resolution could not find a matching entry on disk.
    FileNotFoundCaseInsensitive {
        /// The original path requested by the caller.
        requested: PathBuf,
        /// The directory that was searched at the failure point.
        searched_in: PathBuf,
    },
}

impl Display for DataDirectoryError {
    /// Formats a human-readable message describing the failure.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::InvalidRelativePath { requested } => write!(
                f,
                "path must be a relative path inside the data directory: {}",
                requested.display()
            ),
            Self::FileNotFoundCaseInsensitive {
                requested,
                searched_in,
            } => write!(
                f,
                "could not find '{}' (case-insensitive) in '{}'",
                requested.display(),
                searched_in.display()
            ),
        }
    }
}

impl Error for DataDirectoryError {
    /// Surfaces the underlying I/O error when the variant carries one.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

impl From<std::io::Error> for DataDirectoryError {
    /// Wraps an `io::Error` in the `Io` variant for use with the `?` operator.
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Filesystem location holding the original Jill of the Jungle data files.
///
/// Provides case-insensitive path resolution (matching the lookup behaviour
/// the original engine relied on under DOS) and a convenience helper to open
/// resolved files as a [`ByteReader`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataDirectory(
    /// Filesystem path to the data directory root.
    PathBuf,
);

impl DataDirectory {
    /// Creates a new data directory wrapping `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Returns the data directory's root as a `&Path`.
    pub fn as_path(&self) -> &Path {
        &self.0
    }

    /// Consumes the wrapper and returns the owned root `PathBuf`.
    pub fn into_inner(self) -> PathBuf {
        self.0
    }

    /// Resolves a relative path inside the data directory using a
    /// case-insensitive walk over each component.
    ///
    /// Rejects absolute paths, parent (`..`) components, root components, and
    /// prefix components; the resulting path is guaranteed to live inside the
    /// directory tree rooted at `self.0`.
    pub fn resolve_path_case_insensitive(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<PathBuf, DataDirectoryError> {
        let requested = relative_path.as_ref();
        if requested.is_absolute() {
            return Err(DataDirectoryError::InvalidRelativePath {
                requested: requested.to_path_buf(),
            });
        }

        let mut current = self.0.clone();
        let mut has_normal_component = false;

        for component in requested.components() {
            match component {
                Component::CurDir => continue,
                Component::Normal(segment) => {
                    has_normal_component = true;
                    let next =
                        find_case_insensitive_entry(&current, segment)?.ok_or_else(|| {
                            DataDirectoryError::FileNotFoundCaseInsensitive {
                                requested: requested.to_path_buf(),
                                searched_in: current.clone(),
                            }
                        })?;
                    current = next;
                }
                Component::ParentDir | Component::Prefix(_) | Component::RootDir => {
                    return Err(DataDirectoryError::InvalidRelativePath {
                        requested: requested.to_path_buf(),
                    });
                }
            }
        }

        if !has_normal_component {
            return Err(DataDirectoryError::InvalidRelativePath {
                requested: requested.to_path_buf(),
            });
        }

        Ok(current)
    }

    /// Opens a [`ByteReader`] over the resolved file inside the data
    /// directory. The path is resolved case-insensitively before reading.
    pub fn open_reader(
        &self,
        relative_path: impl AsRef<Path>,
    ) -> Result<ByteReader, DataDirectoryError> {
        let path = self.resolve_path_case_insensitive(relative_path)?;
        let bytes = fs::read(path)?;
        Ok(ByteReader::from_bytes(bytes))
    }
}

/// Walks `directory` and returns the first entry whose file name matches
/// `requested_name` case-insensitively, or `None` if no match is found.
fn find_case_insensitive_entry(
    directory: &Path,
    requested_name: &OsStr,
) -> Result<Option<PathBuf>, DataDirectoryError> {
    let requested_name_str = requested_name.to_string_lossy();

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let candidate_name = entry.file_name();
        if candidate_name
            .to_string_lossy()
            .eq_ignore_ascii_case(&requested_name_str)
        {
            return Ok(Some(entry.path()));
        }
    }

    Ok(None)
}

/// Cursor-based reader over an owned byte buffer, with helpers for the
/// little-endian integer reads used throughout the OpenJill data format.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteReader {
    /// The full byte buffer owned by the reader.
    bytes: Vec<u8>,
    /// Current cursor offset into `bytes`.
    offset: usize,
}

impl ByteReader {
    /// Creates a reader over the supplied bytes, starting at offset zero.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            offset: 0,
        }
    }

    /// Returns the total length of the underlying byte buffer.
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Returns `true` when the underlying byte buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Returns the current cursor offset.
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Moves the cursor to `offset`.
    ///
    /// Returns [`ByteReaderError::InvalidSeek`] when `offset` lies past the
    /// end of the buffer.
    pub fn seek(&mut self, offset: usize) -> Result<(), ByteReaderError> {
        if offset > self.bytes.len() {
            return Err(ByteReaderError::InvalidSeek {
                requested: offset,
                len: self.bytes.len(),
            });
        }

        self.offset = offset;
        Ok(())
    }

    /// Advances the cursor by `count` bytes.
    ///
    /// Returns [`ByteReaderError::OffsetOverflow`] when the addition would
    /// overflow `usize`, and [`ByteReaderError::InvalidSeek`] when the
    /// resulting offset would lie past the end of the buffer.
    pub fn skip(&mut self, count: usize) -> Result<(), ByteReaderError> {
        let target = self
            .offset
            .checked_add(count)
            .ok_or(ByteReaderError::OffsetOverflow {
                offset: self.offset,
                delta: count,
            })?;

        self.seek(target)
    }

    /// Reads a single unsigned 8-bit integer and advances the cursor.
    pub fn read_u8(&mut self) -> Result<u8, ByteReaderError> {
        Ok(self.read_exact::<1>("read unsigned 8-bit integer")?[0])
    }

    /// Reads a single signed 8-bit integer and advances the cursor.
    pub fn read_i8(&mut self) -> Result<i8, ByteReaderError> {
        Ok(self.read_u8()? as i8)
    }

    /// Reads an unsigned 16-bit little-endian integer and advances the cursor.
    pub fn read_u16_le(&mut self) -> Result<u16, ByteReaderError> {
        let bytes = self.read_exact::<2>("read unsigned 16-bit little-endian integer")?;
        Ok(u16::from_le_bytes(bytes))
    }

    /// Reads a signed 16-bit little-endian integer and advances the cursor.
    pub fn read_i16_le(&mut self) -> Result<i16, ByteReaderError> {
        let bytes = self.read_exact::<2>("read signed 16-bit little-endian integer")?;
        Ok(i16::from_le_bytes(bytes))
    }

    /// Reads an unsigned 32-bit little-endian integer and advances the cursor.
    pub fn read_u32_le(&mut self) -> Result<u32, ByteReaderError> {
        let bytes = self.read_exact::<4>("read unsigned 32-bit little-endian integer")?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Reads a signed 32-bit little-endian integer and advances the cursor.
    pub fn read_i32_le(&mut self) -> Result<i32, ByteReaderError> {
        let bytes = self.read_exact::<4>("read signed 32-bit little-endian integer")?;
        Ok(i32::from_le_bytes(bytes))
    }

    /// Copies the next `N` bytes into a fixed-size array and advances the
    /// cursor by `N`.
    ///
    /// Returns [`ByteReaderError::OffsetOverflow`] when the offset arithmetic
    /// would overflow, and [`ByteReaderError::UnexpectedEof`] when fewer than
    /// `N` bytes remain in the buffer.
    fn read_exact<const N: usize>(
        &mut self,
        operation: &'static str,
    ) -> Result<[u8; N], ByteReaderError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(ByteReaderError::OffsetOverflow {
                offset: self.offset,
                delta: N,
            })?;

        if end > self.bytes.len() {
            return Err(ByteReaderError::UnexpectedEof {
                operation,
                offset: self.offset,
                requested: N,
                len: self.bytes.len(),
            });
        }

        let mut out = [0; N];
        out.copy_from_slice(&self.bytes[self.offset..end]);
        self.offset = end;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::{ByteReader, ByteReaderError, DataDirectory, DataDirectoryError};
    use assert2::check;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Unit under test: every `ByteReader::read_*` integer helper.
    ///
    /// Preconditions: a hand-crafted little-endian byte sequence that packs
    /// values for `u8`, `i8`, `u16`, `i16`, `u32`, and `i32` back-to-back.
    ///
    /// Invariants asserted: each helper decodes the expected value, the
    /// signed/unsigned interpretation matches, and the cursor advances by the
    /// exact width of each type so the final offset equals the buffer length.
    #[test]
    fn reads_signed_and_unsigned_little_endian_values() {
        let mut reader = ByteReader::from_bytes([
            0xfe, // u8 = 254
            0x80, // i8 = -128
            0x34, 0x12, // u16 = 0x1234
            0xcc, 0xed, // i16 = -4660
            0x78, 0x56, 0x34, 0x12, // u32 = 0x12345678
            0x88, 0xa9, 0xcb, 0xed, // i32 = -305419896
        ]);

        check!(let Ok(v) = reader.read_u8() && v == 254);
        check!(let Ok(v) = reader.read_i8() && v == -128);
        check!(let Ok(v) = reader.read_u16_le() && v == 0x1234);
        check!(let Ok(v) = reader.read_i16_le() && v == -4_660);
        check!(let Ok(v) = reader.read_u32_le() && v == 0x1234_5678);
        check!(let Ok(v) = reader.read_i32_le() && v == -305_419_896);
        check!(reader.offset() == reader.len());
    }

    /// Unit under test: cursor manipulation and bounds-checking on `ByteReader`.
    ///
    /// Preconditions: a four-byte buffer is wrapped in a fresh reader.
    ///
    /// Invariants asserted: `len`/`offset` reflect the initial state; `skip`
    /// and `seek` move the cursor exactly when in-bounds; reads after EOF
    /// return `UnexpectedEof`; out-of-bounds `skip` and `seek` return
    /// `InvalidSeek` with the requested offset and buffer length surfaced in
    /// the error.
    #[test]
    fn seek_skip_offset_length_and_eof_are_tracked() {
        let mut reader = ByteReader::from_bytes([1, 2, 3, 4]);

        check!(reader.len() == 4);
        check!(reader.offset() == 0);

        check!(let Ok(()) = reader.skip(2));
        check!(reader.offset() == 2);

        check!(let Ok(()) = reader.seek(1));
        check!(reader.offset() == 1);
        check!(let Ok(v) = reader.read_u16_le() && v == 0x0302);
        check!(reader.offset() == 3);

        check!(let Ok(()) = reader.seek(4));
        check!(
            let Err(eof) = reader.read_u8()
                && eof == ByteReaderError::UnexpectedEof {
                    operation: "read unsigned 8-bit integer",
                    offset: 4,
                    requested: 1,
                    len: 4,
                }
        );

        check!(
            let Err(invalid_skip) = reader.skip(1)
                && invalid_skip == ByteReaderError::InvalidSeek {
                    requested: 5,
                    len: 4,
                }
        );

        check!(
            let Err(invalid_seek) = reader.seek(5)
                && invalid_seek == ByteReaderError::InvalidSeek {
                    requested: 5,
                    len: 4,
                }
        );
    }

    /// Unit under test: `DataDirectory::resolve_path_case_insensitive` and
    /// `DataDirectory::open_reader`.
    ///
    /// Preconditions: a temporary directory is populated with `SubDir/JILL1.DMA`
    /// (mixed case) and a [`DataDirectory`] is constructed over it.
    ///
    /// Invariants asserted: a lowercase `subdir/jill1.dma` lookup resolves to
    /// the real mixed-case path on disk; opening it returns a working
    /// `ByteReader`; a missing file produces a
    /// `FileNotFoundCaseInsensitive` error rather than an I/O error or panic.
    #[test]
    fn resolves_case_insensitive_file_paths() {
        let data_dir = TempDirGuard::new("openjill-data-case-insensitive");
        let data_dir_path = data_dir.path();
        let nested_dir = data_dir_path.join("SubDir");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        let file_path = nested_dir.join("JILL1.DMA");
        fs::write(&file_path, [0x34, 0x12]).expect("write test file");

        let directory = DataDirectory::new(data_dir_path.to_path_buf());
        check!(
            let Ok(resolved) = directory.resolve_path_case_insensitive("subdir/jill1.dma")
                && resolved == file_path
        );

        let mut reader = directory
            .open_reader("subdir/jill1.dma")
            .expect("reader should open resolved path");
        check!(let Ok(v) = reader.read_u16_le() && v == 0x1234);

        check!(
            let Err(DataDirectoryError::FileNotFoundCaseInsensitive { .. }) =
                directory.resolve_path_case_insensitive("subdir/does-not-exist.dma")
        );
    }

    /// Owned temporary directory that removes itself on drop, used by the
    /// path-resolution test to keep the real filesystem clean across runs.
    struct TempDirGuard(
        /// Filesystem path to the temporary directory.
        PathBuf,
    );

    impl TempDirGuard {
        /// Creates a temporary directory whose name combines `prefix` with a
        /// nanosecond timestamp to avoid collisions across parallel runs.
        fn new(prefix: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock should be after epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
            fs::create_dir_all(&path).expect("create temp directory");
            Self(path)
        }

        /// Returns the on-disk path of the temporary directory.
        fn path(&self) -> &PathBuf {
            &self.0
        }
    }

    impl Drop for TempDirGuard {
        /// Best-effort recursive deletion of the temporary directory.
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
