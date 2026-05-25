//! DMA export stubs.

use crate::Row;
use openjill_data::dma::DmaFile;

/// Converts parsed `JILL.DMA` metadata into tabular export rows.
pub fn file_to_rows(_file: &DmaFile) -> Vec<Row> {
    unimplemented!("DMA export wiring lands in a follow-up issue")
}
