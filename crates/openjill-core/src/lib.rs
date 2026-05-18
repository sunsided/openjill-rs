#![forbid(unsafe_code)]

use openjill_data::DataDirectory;

#[derive(Clone, Debug)]
pub struct CoreState {
    pub data_directory: DataDirectory,
}

impl CoreState {
    pub fn new(data_directory: DataDirectory) -> Self {
        Self { data_directory }
    }
}
