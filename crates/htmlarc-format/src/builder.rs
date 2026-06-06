use crate::{archive::HtmlArchive, entry::HtmlEntry, error::ArchiveErr, writer::ArchiveWriter};
use htmlarc_dom::prelude::HtmlDoc;
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
        // The BTreeSet already yields entries sorted and unique; stream them through the writer
        // so there is exactly one serialization path and one on-disk format.
        let mut writer = ArchiveWriter::create(path)?;
        for entry in &self.entries {
            writer.push_entry(entry)?;
        }
        writer.finish()
    }
}
