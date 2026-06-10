use crate::{archive::HtmlArchive, entry::HtmlEntry, error::ArchiveErr, writer::ArchiveWriter};
use htmlarc_dom::prelude::HtmlDoc;
use std::{collections::HashSet, path::Path};

/// Builds an [`HtmlArchive`] by accumulating documents **in insertion order** (deduping by key,
/// first wins). Insertion order is the order documents are grouped into bundles, so the builder
/// and the streaming [`ArchiveWriter`] produce identical bundle boundaries for the same key set.
#[derive(Default)]
pub struct HtmlArchiveBuilder {
    entries: Vec<HtmlEntry>,
    seen: HashSet<String>,
}

impl HtmlArchiveBuilder {
    /// Add a parsed document. Duplicate keys are skipped (first wins), matching the writer's
    /// streaming dedup.
    pub fn add_html(&mut self, key: String, html: HtmlDoc) {
        if self.seen.insert(key.clone()) {
            self.entries.push(HtmlEntry::new(key, html));
        }
    }

    /// Collect the (insertion-ordered) entries into an in-memory archive for querying.
    pub fn build(self) -> HtmlArchive {
        HtmlArchive::from_vec(self.entries)
    }

    pub fn write_to<P: AsRef<Path>>(self, path: P) -> Result<(), ArchiveErr> {
        // Stream the entries through the writer in insertion order so there is exactly one
        // serialization path, one on-disk format, and one bundle layout.
        let mut writer = ArchiveWriter::create(path)?;
        for entry in &self.entries {
            writer.push_entry(entry)?;
        }
        writer.finish()
    }
}
