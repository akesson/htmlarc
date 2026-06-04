use super::DataManager;
use anyhow::{Context, Result};
use htmlarc_format::HtmlArchive;
use std::{path::Path, sync::Arc};

pub struct FileData;

impl DataManager for FileData {
    fn create_list_arch(&self, source: &Path) -> Result<Arc<HtmlArchive>> {
        let archive = HtmlArchive::open(source)
            .with_context(|| format!("opening source {}", source.display()))?;
        Ok(Arc::new(archive))
    }

    fn create_diff_arch(&self, source: &Path) -> Result<HtmlArchive> {
        HtmlArchive::open(source).with_context(|| format!("opening source {}", source.display()))
    }
}
