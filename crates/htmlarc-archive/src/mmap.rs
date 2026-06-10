use std::ops::{Index, Range};
use std::path::Path;

use memmap2::Mmap;
use rkyv::rancor::Error;

use crate::bundle::ArchivedBundleTable;
use crate::doc_table::{self, ArchivedDocTable, ArchivedSortIndex};
use crate::entry::ArchivedHtmlEntry;
use crate::error::ArchiveErr;
use crate::trailer::Trailer;

/// A memory-mapped v4 `.htmlarc` archive, queried **lazily and zero-copy**: opening maps the file
/// and validates only the footer (trailer + doc table + sort index + bundle table); each
/// document's blob is validated and read in place the moment it is fetched — never deserialized,
/// never all faulted in at once.
///
/// Metadata (`len`, `keys`, checksum-by-key, bundle ranges) is served straight from the footer
/// without touching any blob. Bulk iteration proceeds **bundle→doc** (doc-table order); keyed
/// lookup binary-searches the sort-index permutation.
///
/// # Caveat
/// The file backing the mapping must not be truncated or rewritten in place while an
/// `MmapArchive` is open — doing so can raise `SIGBUS` on access. [`crate::ArchiveWriter`] writes
/// to a temp path and renames, so it never corrupts a mapping of a previous build.
pub struct MmapArchive {
    mmap: Mmap,
    doc_table_offset: usize,
    doc_table_len: usize,
    bundle_table_offset: usize,
    bundle_table_len: usize,
    sort_index_offset: usize,
    sort_index_len: usize,
}

impl MmapArchive {
    /// Memory-map a v4 `.htmlarc` and validate its footer (trailer bounds + the doc table, sort
    /// index, and bundle table). Individual document blobs are validated lazily on access.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let file = std::fs::File::open(path.as_ref()).map_err(ArchiveErr::FileRead)?;
        // SAFETY: the mapping is only ever read; the caller must not mutate the file while it is
        // open (see the type-level caveat).
        let mmap = unsafe { Mmap::map(&file) }.map_err(ArchiveErr::FileRead)?;

        crate::header::validate_header(&mmap)?;
        let trailer = Trailer::read_from_tail(&mmap)?;
        let doc_table_offset = trailer.doc_table_offset as usize;
        let doc_table_len = trailer.doc_table_len as usize;
        let bundle_table_offset = trailer.bundle_table_offset as usize;
        let bundle_table_len = trailer.bundle_table_len as usize;
        let sort_index_offset = trailer.sort_index_offset as usize;
        let sort_index_len = trailer.sort_index_len as usize;

        // Validate the doc table first (the sort index dereferences into it), then the sort index,
        // then the bundle table — safe rkyv access (bytecheck). A corrupt/misaligned footer
        // becomes an `Err` here rather than UB on the first query.
        let doc_slice = slice(&mmap, doc_table_offset, doc_table_len, "doc table")?;
        let doc_table = rkyv::access::<ArchivedDocTable, Error>(doc_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;
        let doc_count = doc_table.len();

        let sort_slice = slice(&mmap, sort_index_offset, sort_index_len, "sort index")?;
        let sort_index = rkyv::access::<ArchivedSortIndex, Error>(sort_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        // bytecheck validates structure, not values: a corrupt permutation entry would index
        // out of the doc table and panic at query time. Reject it up front.
        if sort_index.len() != doc_count {
            return Err(ArchiveErr::Validate(
                "sort index length does not match the doc table".into(),
            ));
        }
        for p in sort_index.iter() {
            if (p.to_native() as usize) >= doc_count {
                return Err(ArchiveErr::Validate(
                    "sort index entry out of range for the doc table".into(),
                ));
            }
        }

        let bt_slice = slice(&mmap, bundle_table_offset, bundle_table_len, "bundle table")?;
        let bundle_table = rkyv::access::<ArchivedBundleTable, Error>(bt_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        // Bundles must tile the doc table contiguously: a malformed table would let a bundle
        // range index out of bounds. Verify it covers [0, doc_count) exactly.
        let mut expected_start = 0usize;
        for desc in bundle_table.iter() {
            if desc.doc_start.to_native() as usize != expected_start {
                return Err(ArchiveErr::Validate(
                    "bundle table is not contiguous over the doc table".into(),
                ));
            }
            expected_start += desc.doc_count.to_native() as usize;
        }
        if expected_start != doc_count {
            return Err(ArchiveErr::Validate(
                "bundle table does not cover the whole doc table".into(),
            ));
        }

        Ok(Self {
            mmap,
            doc_table_offset,
            doc_table_len,
            bundle_table_offset,
            bundle_table_len,
            sort_index_offset,
            sort_index_len,
        })
    }

    fn doc_table(&self) -> &ArchivedDocTable {
        // SAFETY: `open` validated this exact slice and the mapping is immutable.
        let s = &self.mmap[self.doc_table_offset..self.doc_table_offset + self.doc_table_len];
        unsafe { rkyv::access_unchecked::<ArchivedDocTable>(s) }
    }

    fn sort_index(&self) -> &ArchivedSortIndex {
        let s = &self.mmap[self.sort_index_offset..self.sort_index_offset + self.sort_index_len];
        unsafe { rkyv::access_unchecked::<ArchivedSortIndex>(s) }
    }

    fn bundle_table(&self) -> &ArchivedBundleTable {
        let s =
            &self.mmap[self.bundle_table_offset..self.bundle_table_offset + self.bundle_table_len];
        unsafe { rkyv::access_unchecked::<ArchivedBundleTable>(s) }
    }

    /// Validate and read the doc blob at `(offset, len)` in place. Uses *safe* `access` so a
    /// corrupt blob surfaces as an `Err` instead of UB.
    fn blob(&self, offset: u64, len: u64) -> Result<&ArchivedHtmlEntry, ArchiveErr> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| ArchiveErr::Validate("blob offset/len overflow".into()))?;
        let slice = self
            .mmap
            .get(offset as usize..end as usize)
            .ok_or_else(|| ArchiveErr::Validate("document blob lies outside the file".into()))?;
        rkyv::access::<ArchivedHtmlEntry, Error>(slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))
    }

    pub fn len(&self) -> usize {
        self.doc_table().len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc_table().is_empty()
    }

    /// Number of bundles.
    pub fn bundle_count(&self) -> usize {
        self.bundle_table().len()
    }

    /// The half-open flat (doc-table) position range covered by bundle `b`.
    pub fn bundle_range(&self, b: usize) -> Range<usize> {
        let desc = &self.bundle_table()[b];
        let start = desc.doc_start.to_native() as usize;
        start..start + desc.doc_count.to_native() as usize
    }

    /// Look an entry up by key. `Ok(None)` = absent; `Err` = the matching blob failed validation.
    /// The binary search only reads the footer; the blob is touched only on a hit.
    pub fn get(&self, key: &str) -> Result<Option<&ArchivedHtmlEntry>, ArchiveErr> {
        match doc_table::find(self.doc_table(), self.sort_index(), key) {
            Some(d) => Ok(Some(self.blob(d.offset.to_native(), d.len.to_native())?)),
            None => Ok(None),
        }
    }

    /// The checksum for `key`, read straight from the doc table (no blob access).
    pub fn checksum_for_key(&self, key: &str) -> Option<u64> {
        doc_table::find(self.doc_table(), self.sort_index(), key).map(|d| d.checksum.to_native())
    }

    /// The flat (bundle→doc) position of `key`, via the sort index — footer-only, no blob access.
    /// Lets a keyed search resolve a word-list straight to positions instead of scanning.
    pub fn position_for_key(&self, key: &str) -> Option<usize> {
        doc_table::find_index(self.doc_table(), self.sort_index(), key)
    }

    /// The key at positional index `i` (bundle→doc order), no blob access.
    pub fn key_at(&self, i: usize) -> &str {
        self.doc_table()[i].key.as_str()
    }

    /// The checksum at positional index `i` (bundle→doc order), no blob access.
    pub fn checksum_at(&self, i: usize) -> u64 {
        self.doc_table()[i].checksum.to_native()
    }

    pub fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        self.doc_table().iter().map(move |d| {
            self.blob(d.offset.to_native(), d.len.to_native())
                .expect("corrupt document blob during iteration")
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.doc_table().iter().map(|d| d.key.as_str())
    }
}

/// Slice a region with a bounds check, turning an out-of-range footer offset into an `Err`.
fn slice<'a>(
    mmap: &'a [u8],
    offset: usize,
    len: usize,
    what: &str,
) -> Result<&'a [u8], ArchiveErr> {
    mmap.get(offset..offset + len)
        .ok_or_else(|| ArchiveErr::Validate(format!("{what} range lies outside the file")))
}

impl Index<usize> for MmapArchive {
    type Output = ArchivedHtmlEntry;

    /// Positional access (bundle→doc order). Panics on a corrupt blob — `Index` cannot return a
    /// `Result`, and a bad blob at a valid index means the archive is corrupt.
    fn index(&self, index: usize) -> &Self::Output {
        let d = &self.doc_table()[index];
        self.blob(d.offset.to_native(), d.len.to_native())
            .expect("corrupt document blob")
    }
}

impl crate::Archive for MmapArchive {
    type Entry = ArchivedHtmlEntry;

    fn len(&self) -> usize {
        self.doc_table().len()
    }

    fn get(&self, key: &str) -> Result<Option<&ArchivedHtmlEntry>, ArchiveErr> {
        MmapArchive::get(self, key)
    }

    fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        MmapArchive::entries(self)
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        MmapArchive::keys(self)
    }
}
