#![forbid(unsafe_code)]

use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{Display, Formatter};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ByteReaderError {
    UnexpectedEof {
        operation: &'static str,
        offset: usize,
        requested: usize,
        len: usize,
    },
    InvalidSeek {
        requested: usize,
        len: usize,
    },
    OffsetOverflow {
        offset: usize,
        delta: usize,
    },
}

impl Display for ByteReaderError {
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

#[derive(Debug)]
pub enum DataDirectoryError {
    Io(std::io::Error),
    InvalidRelativePath {
        requested: PathBuf,
    },
    FileNotFoundCaseInsensitive {
        requested: PathBuf,
        searched_in: PathBuf,
    },
}

impl Display for DataDirectoryError {
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
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        if let Self::Io(error) = self {
            Some(error)
        } else {
            None
        }
    }
}

impl From<std::io::Error> for DataDirectoryError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataDirectory(PathBuf);

impl DataDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn into_inner(self) -> PathBuf {
        self.0
    }

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
                    let next = find_case_insensitive_entry(&current, segment)?.ok_or_else(|| {
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

    pub fn open_reader(&self, relative_path: impl AsRef<Path>) -> Result<ByteReader, DataDirectoryError> {
        let path = self.resolve_path_case_insensitive(relative_path)?;
        let bytes = fs::read(path)?;
        Ok(ByteReader::from_bytes(bytes))
    }
}

fn find_case_insensitive_entry(
    directory: &Path,
    requested_name: &OsStr,
) -> Result<Option<PathBuf>, DataDirectoryError> {
    let requested_name = requested_name.to_string_lossy();

    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let candidate_name = entry.file_name();
        if candidate_name
            .to_string_lossy()
            .eq_ignore_ascii_case(&requested_name)
        {
            return Ok(Some(entry.path()));
        }
    }

    Ok(None)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ByteReader {
    bytes: Vec<u8>,
    offset: usize,
}

impl ByteReader {
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            bytes: bytes.into(),
            offset: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn offset(&self) -> usize {
        self.offset
    }

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

    pub fn read_u8(&mut self) -> Result<u8, ByteReaderError> {
        Ok(self.read_exact::<1>("read unsigned 8-bit integer")?[0])
    }

    pub fn read_i8(&mut self) -> Result<i8, ByteReaderError> {
        Ok(self.read_u8()? as i8)
    }

    pub fn read_u16_le(&mut self) -> Result<u16, ByteReaderError> {
        let bytes = self.read_exact::<2>("read unsigned 16-bit little-endian integer")?;
        Ok(u16::from_le_bytes(bytes))
    }

    pub fn read_i16_le(&mut self) -> Result<i16, ByteReaderError> {
        let bytes = self.read_exact::<2>("read signed 16-bit little-endian integer")?;
        Ok(i16::from_le_bytes(bytes))
    }

    pub fn read_u32_le(&mut self) -> Result<u32, ByteReaderError> {
        let bytes = self.read_exact::<4>("read unsigned 32-bit little-endian integer")?;
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_i32_le(&mut self) -> Result<i32, ByteReaderError> {
        let bytes = self.read_exact::<4>("read signed 32-bit little-endian integer")?;
        Ok(i32::from_le_bytes(bytes))
    }

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
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

        assert_eq!(reader.read_u8().expect("u8 read should succeed"), 254);
        assert_eq!(reader.read_i8().expect("i8 read should succeed"), -128);
        assert_eq!(
            reader.read_u16_le().expect("u16 read should succeed"),
            0x1234
        );
        assert_eq!(
            reader.read_i16_le().expect("i16 read should succeed"),
            -4_660
        );
        assert_eq!(
            reader.read_u32_le().expect("u32 read should succeed"),
            0x1234_5678
        );
        assert_eq!(
            reader.read_i32_le().expect("i32 read should succeed"),
            -305_419_896
        );
        assert_eq!(reader.offset(), reader.len());
    }

    #[test]
    fn seek_skip_offset_length_and_eof_are_tracked() {
        let mut reader = ByteReader::from_bytes([1, 2, 3, 4]);

        assert_eq!(reader.len(), 4);
        assert_eq!(reader.offset(), 0);

        reader.skip(2).expect("skip should succeed");
        assert_eq!(reader.offset(), 2);

        reader.seek(1).expect("seek should succeed");
        assert_eq!(reader.offset(), 1);
        assert_eq!(
            reader.read_u16_le().expect("u16 read should succeed"),
            0x0302
        );
        assert_eq!(reader.offset(), 3);

        reader.seek(4).expect("seek to EOF should be allowed");
        let eof = reader.read_u8().expect_err("read at EOF should fail");
        assert_eq!(
            eof,
            ByteReaderError::UnexpectedEof {
                operation: "read unsigned 8-bit integer",
                offset: 4,
                requested: 1,
                len: 4,
            }
        );

        let invalid_skip = reader.skip(1).expect_err("skip beyond EOF should fail");
        assert_eq!(
            invalid_skip,
            ByteReaderError::InvalidSeek {
                requested: 5,
                len: 4,
            }
        );

        let invalid_seek = reader.seek(5).expect_err("seek beyond EOF should fail");
        assert_eq!(
            invalid_seek,
            ByteReaderError::InvalidSeek {
                requested: 5,
                len: 4,
            }
        );
    }

    #[test]
    fn resolves_case_insensitive_file_paths() {
        let data_dir = create_temp_dir("openjill-data-case-insensitive");
        let nested_dir = data_dir.join("SubDir");
        fs::create_dir_all(&nested_dir).expect("create nested dir");

        let file_path = nested_dir.join("JILL1.DMA");
        fs::write(&file_path, [0x34, 0x12]).expect("write test file");

        let directory = DataDirectory::new(data_dir.clone());
        let resolved = directory
            .resolve_path_case_insensitive("subdir/jill1.dma")
            .expect("case-insensitive path should resolve");
        assert_eq!(resolved, file_path);

        let mut reader = directory
            .open_reader("subdir/jill1.dma")
            .expect("reader should open resolved path");
        assert_eq!(reader.read_u16_le().expect("u16 read should succeed"), 0x1234);

        let missing = directory
            .resolve_path_case_insensitive("subdir/does-not-exist.dma")
            .expect_err("missing file should fail");
        assert!(matches!(
            missing,
            DataDirectoryError::FileNotFoundCaseInsensitive { .. }
        ));

        fs::remove_dir_all(&data_dir).expect("cleanup temp dir");
    }

    fn create_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{nanos}"));
        fs::create_dir_all(&path).expect("create temp directory");
        path
    }
}
