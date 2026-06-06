use std::ops::Index;
use std::path::Path;

use memmap2::Mmap;
use rkyv::rancor::Error;
use unicode_segmentation::UnicodeSegmentation;

use crate::entry::{ArchivedHtmlEntry, HtmlEntry};
use crate::error::ArchiveErr;
use crate::header::payload_offset;

/// The rkyv-archived form of the archive's `Vec<HtmlEntry>`.
type ArchivedEntries = rkyv::Archived<Vec<HtmlEntry>>;

/// A memory-mapped `.htmlarc` archive, queried **fully zero-copy**: opening only
/// maps the file and validates it once; every entry — keys, checksums, and the
/// flat DOM — is read in place out of the mapping with no deserialization.
///
/// Because the archived DOM (`ArchivedDomInner`) implements
/// [`htmlarc_dom::prelude::DomRead`]/`DomRef`, an entry's document is queried and
/// formatted with the exact same API as an owned one.
///
/// # Caveat
/// The file backing the mapping must not be truncated or rewritten in place while
/// an `MmapArchive` is open — doing so can raise `SIGBUS` on access. Write new
/// archives to a fresh path (or temp-file + rename) rather than overwriting one
/// that may be mapped.
pub struct MmapArchive {
    mmap: Mmap,
    offset: usize,
}

impl MmapArchive {
    /// Memory-map and validate a `.htmlarc` file (modern or legacy/header-less).
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let file = std::fs::File::open(path.as_ref()).map_err(ArchiveErr::FileRead)?;
        // SAFETY: the mapping is only ever read; the caller must not mutate the file
        // while it is open (see the type-level caveat).
        let mmap = unsafe { Mmap::map(&file) }.map_err(ArchiveErr::FileRead)?;

        let offset = payload_offset(&mmap)?;

        // Validate once, up front (safe rkyv access with bytecheck): a corrupt,
        // misaligned, or truncated archive becomes an `Err` here rather than UB on
        // the first query.
        rkyv::access::<ArchivedEntries, Error>(&mmap[offset..])
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        Ok(Self { mmap, offset })
    }

    fn archived(&self) -> &ArchivedEntries {
        // SAFETY: `open` validated this exact slice and the mapping is immutable, so
        // the cheap (no-revalidation) access is sound and avoids re-checking on every
        // query.
        unsafe { rkyv::access_unchecked::<ArchivedEntries>(&self.mmap[self.offset..]) }
    }

    pub fn len(&self) -> usize {
        self.archived().len()
    }

    pub fn is_empty(&self) -> bool {
        self.archived().is_empty()
    }

    /// Zero-copy binary search by key — mirrors [`crate::HtmlArchive::get`], reading
    /// `key_len`/`key` straight from the mapping.
    pub fn get(&self, key: &str) -> Option<&ArchivedHtmlEntry> {
        let key_len = key.graphemes(true).count() as u16;
        let entries = self.archived();
        let found = entries
            .binary_search_by(|e| {
                e.key_len
                    .to_native()
                    .cmp(&key_len)
                    .then_with(|| e.key.as_str().cmp(key))
            })
            .ok()?;
        Some(&entries[found])
    }

    pub fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        self.archived().iter()
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.archived().iter().map(|e| e.key.as_str())
    }
}

impl Index<usize> for MmapArchive {
    type Output = ArchivedHtmlEntry;

    fn index(&self, index: usize) -> &Self::Output {
        &self.archived()[index]
    }
}

impl crate::Archive for MmapArchive {
    type Entry = ArchivedHtmlEntry;

    fn len(&self) -> usize {
        self.archived().len()
    }

    fn get(&self, key: &str) -> Option<&ArchivedHtmlEntry> {
        MmapArchive::get(self, key)
    }

    fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        self.archived().iter()
    }
}
