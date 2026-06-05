use crate::{archive::HtmlArchive, entry::HtmlEntry, error::ArchiveErr};
use fs_err as fs;
use htmlarc_dom::prelude::HtmlDoc;
use rkyv::rancor::Error;
use std::{collections::BTreeSet, path::Path};

#[derive(Default)]
pub struct HtmlArchiveBuilder {
    entries: BTreeSet<HtmlEntry>,
}

impl HtmlArchiveBuilder {
    pub fn add_html(&mut self, key: String, html: HtmlDoc) {
        self.entries.insert(HtmlEntry::new(key, html));
    }

    /// Collect the (sorted) entries into an in-memory archive for querying.
    pub fn build(self) -> HtmlArchive {
        HtmlArchive::from_vec(self.entries.into_iter().collect())
    }

    pub fn write_to<P: AsRef<Path>>(self, path: P) -> Result<(), ArchiveErr> {
        let entries = self.entries.into_iter().collect::<Vec<HtmlEntry>>();
        let data =
            rkyv::to_bytes::<Error>(&entries).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        let header = crate::header::header_bytes();
        let mut out = Vec::with_capacity(header.len() + data.len());
        out.extend_from_slice(&header);
        out.extend_from_slice(&data);
        fs::write(path, out).map_err(ArchiveErr::FileWrite)?;

        Ok(())
    }
}
