//! The footer **directory**: one `DirEntry` per stored document, holding everything
//! needed to find and diff a doc without touching its blob — the key (and its grapheme
//! `key_len`, the primary sort dimension), the DOM `checksum`, and the byte `(offset, len)`
//! of the doc's rkyv blob in the file.
//!
//! The directory is serialized as a single rkyv `Vec<DirEntry>` **sorted by (key_len, key)**,
//! so both the owned and memory-mapped readers binary-search it with the exact comparator
//! the old single-blob format used on `Vec<HtmlEntry>`.

use rkyv::{Archive, Deserialize, Serialize};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Archive, Serialize, Deserialize, Debug, Clone)]
pub(crate) struct DirEntry {
    /// The entry key (e.g. the source file name).
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

/// The rkyv-archived form of the footer directory.
pub(crate) type ArchivedDirectory = rkyv::Archived<Vec<DirEntry>>;

/// Compare a directory key by the (key_len, key) ordering used throughout the format.
pub(crate) fn compare(
    entry_key_len: u16,
    entry_key: &str,
    key_len: u16,
    key: &str,
) -> std::cmp::Ordering {
    entry_key_len.cmp(&key_len).then_with(|| entry_key.cmp(key))
}

/// Binary-search the archived directory for `key`, returning its entry (offset/len/checksum).
pub(crate) fn find<'a>(dir: &'a ArchivedDirectory, key: &str) -> Option<&'a ArchivedDirEntry> {
    let key_len = key.graphemes(true).count() as u16;
    let idx = dir
        .binary_search_by(|d| compare(d.key_len.to_native(), d.key.as_str(), key_len, key))
        .ok()?;
    Some(&dir[idx])
}
