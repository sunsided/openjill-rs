#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

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
}
