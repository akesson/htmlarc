//! Streaming in-place append to an existing `.htmlarc` (ADR 0010).
//!
//! [`ArchiveAppender`] wraps an [`ArchiveWriter`](crate::writer::ArchiveWriter) opened in
//! append mode: new documents stream into the file the moment they are added (memory stays
//! flat regardless of archive size), duplicates dedup against **all** keys — existing and
//! new, first wins — and the metadata table, if the archive carries one, is continued row
//! for row. [`commit`](ArchiveAppender::commit) writes the new footer and makes the tail
//! authoritative again; dropping the appender without committing leaves the file readable
//! as the pre-append archive (the recovery contract staged in the header).

use std::path::Path;

use htmlarc_dom::prelude::HtmlDoc;

use crate::bundle::BUNDLE_CAP;
use crate::error::ArchiveErr;
use crate::meta::{MetaSchema, MetaTableBuilder, MetaValue};
use crate::writer::ArchiveWriter;

/// Appends documents to an existing `.htmlarc` in place. See the module docs.
pub struct ArchiveAppender {
    writer: ArchiveWriter,
    meta: Option<MetaTableBuilder>,
    /// Documents stored since the last bundle seal (appended docs form fresh
    /// [`BUNDLE_CAP`]-sized bundles; old bundles are untouched).
    since_seal: usize,
    appended: usize,
}

impl ArchiveAppender {
    /// Open `path` for appending. Fails on a missing/older-version archive (re-pack to
    /// upgrade). An abandoned earlier append is healed: its garbage tail is overwritten.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let (writer, meta) = ArchiveWriter::append(path)?;
        Ok(Self {
            writer,
            meta: meta.map(MetaTableBuilder::from_table),
            since_seal: 0,
            appended: 0,
        })
    }

    /// The archive's metadata schema, if it carries one. Appended documents may (and, for
    /// non-null values, must) supply rows against exactly this schema.
    pub fn meta_schema(&self) -> Option<&MetaSchema> {
        self.meta.as_ref().map(|m| m.schema())
    }

    /// Append a parsed document. A key that already exists (in the old archive or appended
    /// earlier) is skipped — first wins, like the builder. With a metadata schema present the
    /// document gets an all-null row.
    pub fn add_html(&mut self, key: String, html: HtmlDoc) -> Result<bool, ArchiveErr> {
        self.add_inner(key, html, None)
    }

    /// Append a parsed document with its metadata row (`row[i]` = schema field `i`, `None` =
    /// null). Requires the archive to carry a schema. Returns whether the document was stored
    /// (`false` = duplicate key; the row is dropped with it).
    pub fn add_html_with_meta(
        &mut self,
        key: String,
        html: HtmlDoc,
        row: Vec<Option<MetaValue>>,
    ) -> Result<bool, ArchiveErr> {
        if self.meta.is_none() {
            return Err(ArchiveErr::Validate(
                "archive carries no metadata schema; append without meta".into(),
            ));
        }
        self.add_inner(key, html, Some(row))
    }

    fn add_inner(
        &mut self,
        key: String,
        html: HtmlDoc,
        row: Option<Vec<Option<MetaValue>>>,
    ) -> Result<bool, ArchiveErr> {
        // Validate the row before the document hits the file — after `push` stores it, a row
        // failure could no longer be unwound.
        if let (Some(meta), Some(row)) = (self.meta.as_ref(), row.as_deref()) {
            meta.schema().validate_row(row)?;
        }
        let stored = self.writer.push(key, html)?;
        if !stored {
            return Ok(false);
        }
        if let Some(meta) = self.meta.as_mut() {
            let row = row.unwrap_or_else(|| vec![None; meta.schema().fields.len()]);
            meta.push_row(row)?;
        }
        self.since_seal += 1;
        if self.since_seal == BUNDLE_CAP {
            self.writer.seal_bundle()?;
            self.since_seal = 0;
        }
        self.appended += 1;
        Ok(true)
    }

    /// Number of documents actually appended so far (duplicates excluded).
    pub fn appended(&self) -> usize {
        self.appended
    }

    /// Total documents the archive will hold after commit.
    pub fn doc_count(&self) -> usize {
        self.writer.doc_count()
    }

    /// Seal the tail bundle, write the new footer, and clear the staged recovery offset —
    /// the durable commit point. Appending zero documents is a valid commit (an unchanged
    /// footer is rewritten after the old one).
    pub fn commit(mut self) -> Result<(), ArchiveErr> {
        self.writer
            .set_meta_table(self.meta.take().map(|m| m.finish()));
        self.writer.finish()
    }
}
