#[cfg(not(test))]
mod file_op;
#[cfg(not(test))]
pub use file_op::FileOperator as Operator;

#[cfg(test)]
mod test_op;
#[cfg(test)]
pub use test_op::TestOperator as Operator;

use std::path::Path;

use anyhow::Result;

use crate::source::ArchiveSource;

#[derive(Debug, thiserror::Error)]
pub(super) enum OperationError {
    #[error("'{0}' word not found in '{1}'")]
    GetEntry(String, &'static str),
}

pub trait DataOperator {
    fn write_diff_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn write_list(
        &mut self,
        folder: &Path,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn navigate_diff(
        &mut self,
        indexes: &[usize],
        list_arch: &ArchiveSource,
        diff_arch: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn navigate_list(
        &mut self,
        indexes: &[usize],
        archive: &ArchiveSource,
        raw_html: bool,
    ) -> Result<()>;
    fn list(&mut self, indexes: &[usize], archive: &ArchiveSource);
}
