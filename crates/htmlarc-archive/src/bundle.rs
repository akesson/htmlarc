//! Documents are grouped into [`DocBundle`]s of at most [`BUNDLE_CAP`] entries.
//!
//! A bundle is the unit of *iteration* and *streaming*: a ZIM is parsed into bundles in
//! arrival order, and an in-memory [`HtmlArchive`](crate::HtmlArchive) holds a `Vec<DocBundle>`.
//! Bulk iteration always proceeds bundle→doc. (Keyed lookup, by contrast, goes through a
//! separate global sort index — see [`crate::doc_table`].)
//!
//! On disk a bundle's documents are stored as individual rkyv blobs (so a memory-mapped reader
//! keeps lazy, per-document validation and zero-copy access), followed by one
//! [`BundleStrings`](crate::bundle_strings) block holding every document's text/comment pool,
//! relocated out of the per-document blobs. A [`BundleDesc`] in the footer records which doc-table
//! range belongs to the bundle plus the `(data_offset, data_len)` of that string block.

use rkyv::{Archive, Deserialize, Serialize};

use crate::entry::HtmlEntry;

/// The target number of documents in a single [`DocBundle`]. The ZIM export seals bundles on
/// cluster boundaries once a run reaches this many documents (so a bundle is `BUNDLE_CAP` rounded
/// up to the next cluster); callers that stream documents without sealing (directory pack,
/// re-save of a flat list) get the doc table chunked into bundles of exactly this size.
///
/// 1,000 (not 10,000): the bundle is both the convert in-flight unit and the on-disk
/// compression/symbol-sharing window. Measured at 1k vs 10k (see `docs/decisions/0004`), the
/// on-disk cost is ~+0.4% whole-archive while convert peak memory — which scales directly with
/// per-bundle bytes — drops ~10×. Trivially reversible; raise it if a size-sensitive per-bundle
/// feature (e.g. per-bundle topology compression) ever prefers a larger window.
pub const BUNDLE_CAP: usize = 1_000;

/// A group of up to [`BUNDLE_CAP`] pre-parsed documents, in arrival order.
///
/// This is the in-memory form held by [`HtmlArchive`](crate::HtmlArchive); each entry owns its
/// text pool here (the on-disk per-bundle relocation is applied by [`crate::ArchiveWriter`] on
/// write and undone on load).
#[derive(Default)]
pub struct DocBundle {
    entries: Vec<HtmlEntry>,
}

impl DocBundle {
    pub fn from_entries(entries: Vec<HtmlEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[HtmlEntry] {
        &self.entries
    }

    pub fn iter(&self) -> std::slice::Iter<'_, HtmlEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn into_entries(self) -> Vec<HtmlEntry> {
        self.entries
    }
}

impl std::ops::Index<usize> for DocBundle {
    type Output = HtmlEntry;

    fn index(&self, slot: usize) -> &HtmlEntry {
        &self.entries[slot]
    }
}

/// The footer descriptor for one bundle: the half-open `[doc_start, doc_start + doc_count)`
/// range of the doc table that belongs to it, plus the byte range of the bundle's
/// [`BundleStrings`](crate::bundle_strings) block in the file (the relocated text of every
/// document in the bundle).
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub(crate) struct BundleDesc {
    /// Index of this bundle's first document in the (bundle-ordered) doc table.
    pub doc_start: u32,
    /// Number of documents in this bundle.
    pub doc_count: u32,
    /// Byte offset of this bundle's string block in the file.
    pub data_offset: u64,
    /// Length of this bundle's string block.
    pub data_len: u64,
}

/// The rkyv-archived form of the footer bundle table.
pub(crate) type ArchivedBundleTable = rkyv::Archived<Vec<BundleDesc>>;
