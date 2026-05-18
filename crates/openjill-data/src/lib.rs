#![forbid(unsafe_code)]

use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataDirectory(pub PathBuf);

impl DataDirectory {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }
}
