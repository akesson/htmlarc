use crate::{
    archive::HtmlArchive,
    bundle::BUNDLE_CAP,
    entry::HtmlEntry,
    error::ArchiveErr,
    meta::{MetaSchema, MetaTableBuilder, MetaValue},
    writer::ArchiveWriter,
};
use htmlarc_dom::prelude::HtmlDoc;
use std::{collections::HashSet, path::Path};

/// Builds an [`HtmlArchive`] by accumulating documents **in insertion order** (deduping by key,
/// first wins). Insertion order is the order documents are grouped into bundles, so the builder
/// and the streaming [`ArchiveWriter`] produce identical bundle boundaries for the same key set.
///
/// With a metadata schema ([`set_meta_schema`](Self::set_meta_schema)), each add may carry a
/// typed row (ADR 0009); rows track the dedup — a skipped duplicate never consumes a row, so
/// row `i` always describes stored document `i`.
#[derive(Default)]
pub struct HtmlArchiveBuilder {
    entries: Vec<HtmlEntry>,
    seen: HashSet<String>,
    meta: Option<MetaTableBuilder>,
}

impl HtmlArchiveBuilder {
    /// Declare the metadata schema. Must be called before the first add; every document added
    /// afterwards gets one row (all-null unless supplied via
    /// [`add_html_with_meta`](Self::add_html_with_meta)).
    pub fn set_meta_schema(&mut self, schema: MetaSchema) -> Result<(), ArchiveErr> {
        if !self.entries.is_empty() {
            return Err(ArchiveErr::Validate(
                "metadata schema must be declared before the first document".into(),
            ));
        }
        self.meta = Some(MetaTableBuilder::new(schema));
        Ok(())
    }

    /// The declared metadata schema, if any.
    pub fn meta_schema(&self) -> Option<&MetaSchema> {
        self.meta.as_ref().map(|m| m.schema())
    }

    /// Add a parsed document. Duplicate keys are skipped (first wins), matching the writer's
    /// streaming dedup. With a metadata schema declared, the document gets an all-null row.
    pub fn add_html(&mut self, key: String, html: HtmlDoc) {
        let nulls = self
            .meta
            .as_ref()
            .map(|m| vec![None; m.schema().fields.len()]);
        self.add_inner(key, html, nulls)
            .expect("all-null row always matches the schema");
    }

    /// Add a parsed document with its metadata row (`row[i]` = value for schema field `i`,
    /// `None` = null). Requires a schema; values must match their declared types. A duplicate
    /// key skips both the document and the row.
    pub fn add_html_with_meta(
        &mut self,
        key: String,
        html: HtmlDoc,
        row: Vec<Option<MetaValue>>,
    ) -> Result<(), ArchiveErr> {
        if self.meta.is_none() {
            return Err(ArchiveErr::Validate(
                "no metadata schema declared (set_meta_schema first)".into(),
            ));
        }
        self.add_inner(key, html, Some(row))
    }

    fn add_inner(
        &mut self,
        key: String,
        html: HtmlDoc,
        row: Option<Vec<Option<MetaValue>>>,
    ) -> Result<(), ArchiveErr> {
        if !self.seen.insert(key.clone()) {
            return Ok(());
        }
        // Validate the row *before* storing the entry so a type error leaves the builder
        // consistent (entry count == row count).
        if let (Some(meta), Some(row)) = (self.meta.as_mut(), row) {
            meta.push_row(row).inspect_err(|_| {
                self.seen.remove(&key);
            })?;
        }
        self.entries.push(HtmlEntry::new(key, html));
        Ok(())
    }

    /// Collect the (insertion-ordered) entries into an in-memory archive for querying.
    pub fn build(self) -> HtmlArchive {
        HtmlArchive::from_vec(self.entries).with_meta(self.meta.map(|m| m.finish()))
    }

    pub fn write_to<P: AsRef<Path>>(self, path: P) -> Result<(), ArchiveErr> {
        // Stream the entries through the writer in insertion order so there is exactly one
        // serialization path, one on-disk format, and one bundle layout. Seal a bundle every
        // BUNDLE_CAP documents — matching `HtmlArchive::from_vec`'s grouping — so the boundaries
        // are identical to `build()` and only one bundle's relocated text is buffered at a time.
        let mut writer = ArchiveWriter::create(path)?;
        writer.set_meta_table(self.meta.map(|m| m.finish()));
        for (i, entry) in self.entries.into_iter().enumerate() {
            writer.push_entry(entry)?;
            if (i + 1).is_multiple_of(BUNDLE_CAP) {
                writer.seal_bundle()?;
            }
        }
        writer.finish()
    }
}
