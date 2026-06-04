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
use htmlarc_format::HtmlArchive;

pub trait DataManager {
    /// Open the primary source as an archive (see [`HtmlArchive::open`]).
    fn create_list_arch(&self, source: &Path) -> Result<Arc<HtmlArchive>>;
    /// Open the comparison source for `diff`.
    fn create_diff_arch(&self, source: &Path) -> Result<HtmlArchive>;
}
