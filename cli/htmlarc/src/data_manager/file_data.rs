use super::DataManager;
use crate::source::ArchiveSource;
use anyhow::Result;
use std::{path::Path, sync::Arc};

pub struct FileData;

impl DataManager for FileData {
    fn create_list_arch(&self, source: &Path) -> Result<Arc<ArchiveSource>> {
        Ok(Arc::new(ArchiveSource::open(source)?))
    }

    fn create_diff_arch(&self, source: &Path) -> Result<ArchiveSource> {
        ArchiveSource::open(source)
    }
}
