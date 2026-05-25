//! CFG export stubs.

use crate::Row;
use openjill_data::cfg::CfgFile;

/// Converts parsed `JILL1.CFG` content into tabular export rows.
pub fn file_to_rows(_file: &CfgFile) -> Vec<Row> {
    unimplemented!("CFG export wiring lands in a follow-up issue")
}
