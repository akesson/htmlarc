//! The footer **doc table** and **sort index**.
//!
//! The doc table is one rkyv `Vec<DocEntry>` in **bundle→doc order**: positional document
//! index `i` is exactly `doc_table[i]`, so listing, positional access, and checksum diffing all
//! read straight from the footer (no document blob is touched) and run in bundle order.
//!
//! Keyed lookup ([`crate::HtmlArchive::get`] / [`crate::MmapArchive::get`]) instead binary-searches
//! the **sort index** — a `Vec<u32>` permutation of doc-table positions ordered by `(key_len, key)`.
//! Storing a permutation (4 bytes/doc) keeps `get` at O(log n) without duplicating every key.

use rkyv::{Archive, Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

/// One document's footer metadata — everything needed to find, diff, and locate its blob
/// without deserializing the blob itself.
#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub(crate) struct DocEntry {
    /// The entry key (e.g. the source file name / ZIM title).
    pub key: String,
    /// Grapheme count of `key`, the primary sort/search dimension.
    pub key_len: u16,
    /// Checksum of the stored DOM, for footer-only archive diffing.
    pub checksum: u64,
    /// Byte offset of this doc's rkyv blob in the file (8-byte aligned).
    pub offset: u64,
    /// Exact (unpadded) serialized length of the blob — the slice passed to rkyv `access`.
    pub len: u64,
}

/// The rkyv-archived form of the footer doc table (bundle→doc order).
pub(crate) type ArchivedDocTable = rkyv::Archived<Vec<DocEntry>>;

/// The rkyv-archived form of the footer sort index: a permutation of doc-table positions,
/// ordered by `(key_len, key)`.
pub(crate) type ArchivedSortIndex = rkyv::Archived<Vec<u32>>;

/// The `(key_len, key)` ordering used by the sort index throughout the format.
pub(crate) fn compare(
    a_key_len: u16,
    a_key: &str,
    b_key_len: u16,
    b_key: &str,
) -> std::cmp::Ordering {
    a_key_len.cmp(&b_key_len).then_with(|| a_key.cmp(b_key))
}

/// Grapheme count of a key — the primary sort dimension (matches `DocEntry::key_len`).
pub(crate) fn grapheme_len(key: &str) -> u16 {
    key.graphemes(true).count() as u16
}

/// Binary-search the doc table for `key` **via the sort-index permutation**, returning its
/// **doc-table position** (the flat bundle→doc index). Touches only the footer, no blob.
pub(crate) fn find_index(
    doc_table: &ArchivedDocTable,
    sort_index: &ArchivedSortIndex,
    key: &str,
) -> Option<usize> {
    let key_len = grapheme_len(key);
    let pos = sort_index
        .binary_search_by(|perm| {
            let d = &doc_table[perm.to_native() as usize];
            compare(d.key_len.to_native(), d.key.as_str(), key_len, key)
        })
        .ok()?;
    Some(sort_index[pos].to_native() as usize)
}

/// Binary-search the doc table for `key` **via the sort-index permutation**, returning its
/// archived entry (key/checksum/offset/len). Used by the memory-mapped reader.
pub(crate) fn find<'a>(
    doc_table: &'a ArchivedDocTable,
    sort_index: &ArchivedSortIndex,
    key: &str,
) -> Option<&'a ArchivedDocEntry> {
    find_index(doc_table, sort_index, key).map(|doc| &doc_table[doc])
}
