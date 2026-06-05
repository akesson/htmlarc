#[cfg(not(test))]
mod file_data;
use std::{path::Path, sync::Arc};

#[cfg(not(test))]
pub use file_data::FileData as Manager;

#[cfg(test)]
mod test_data;
#[cfg(test)]
pub use test_data::TestData as Manager;

use anyhow::Result;

use crate::source::ArchiveSource;

pub trait DataManager {
    /// Open the primary source as an archive (memory-mapped if it is a packed
    /// `.htmlarc`, otherwise parsed into owned memory).
    fn create_list_arch(&self, source: &Path) -> Result<Arc<ArchiveSource>>;
    /// Open the comparison source for `diff`.
    fn create_diff_arch(&self, source: &Path) -> Result<ArchiveSource>;
}
