use std::ops::Index;
use std::path::Path;

use memmap2::Mmap;
use rkyv::rancor::Error;

use crate::directory::{self, ArchivedDirectory};
use crate::entry::ArchivedHtmlEntry;
use crate::error::ArchiveErr;
use crate::trailer::Trailer;

/// A memory-mapped v3 `.htmlarc` archive, queried **lazily and zero-copy**: opening maps the
/// file and validates only the footer (trailer + directory); each document's blob is validated
/// and read in place the moment it is fetched — never deserialized, never all faulted in at once.
///
/// Metadata (`len`, `keys`, checksum-by-key) is served straight from the directory without
/// touching any blob, so listing/diffing an archive never pays for the documents themselves.
///
/// Because the archived DOM (`ArchivedDomInner`) implements
/// [`htmlarc_dom::prelude::DomRead`]/`DomRef`, an entry's document is queried and formatted with
/// the exact same API as an owned one.
///
/// # Caveat
/// The file backing the mapping must not be truncated or rewritten in place while an
/// `MmapArchive` is open — doing so can raise `SIGBUS` on access. [`crate::ArchiveWriter`] writes
/// to a temp path and renames, so it never corrupts a mapping of a previous build.
pub struct MmapArchive {
    mmap: Mmap,
    dir_offset: usize,
    dir_len: usize,
}

impl MmapArchive {
    /// Memory-map a v3 `.htmlarc` and validate its footer (trailer bounds + the directory blob).
    /// Individual document blobs are validated lazily on access.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let file = std::fs::File::open(path.as_ref()).map_err(ArchiveErr::FileRead)?;
        // SAFETY: the mapping is only ever read; the caller must not mutate the file while it is
        // open (see the type-level caveat).
        let mmap = unsafe { Mmap::map(&file) }.map_err(ArchiveErr::FileRead)?;

        crate::header::validate_header(&mmap)?;
        let trailer = Trailer::read_from_tail(&mmap)?;
        let dir_offset = trailer.dir_offset as usize;
        let dir_len = trailer.dir_len as usize;

        // Validate the directory once, up front (safe rkyv access with bytecheck). A corrupt or
        // misaligned directory becomes an `Err` here rather than UB on the first query.
        let dir_slice = mmap
            .get(dir_offset..dir_offset + dir_len)
            .ok_or_else(|| ArchiveErr::Validate("directory range lies outside the file".into()))?;
        rkyv::access::<ArchivedDirectory, Error>(dir_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        Ok(Self {
            mmap,
            dir_offset,
            dir_len,
        })
    }

    fn directory(&self) -> &ArchivedDirectory {
        // SAFETY: `open` validated this exact slice and the mapping is immutable.
        let slice = &self.mmap[self.dir_offset..self.dir_offset + self.dir_len];
        unsafe { rkyv::access_unchecked::<ArchivedDirectory>(slice) }
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
        self.directory().len()
    }

    pub fn is_empty(&self) -> bool {
        self.directory().is_empty()
    }

    /// Look an entry up by key. `Ok(None)` = absent; `Err` = the matching blob failed validation.
    /// The binary search itself only reads the directory; the blob is touched only on a hit.
    pub fn get(&self, key: &str) -> Result<Option<&ArchivedHtmlEntry>, ArchiveErr> {
        match directory::find(self.directory(), key) {
            Some(d) => Ok(Some(self.blob(d.offset.to_native(), d.len.to_native())?)),
            None => Ok(None),
        }
    }

    /// The checksum for `key`, read straight from the directory (no blob access).
    pub fn checksum_for_key(&self, key: &str) -> Option<u64> {
        directory::find(self.directory(), key).map(|d| d.checksum.to_native())
    }

    /// The key at positional index `i` (directory order), no blob access.
    pub fn key_at(&self, i: usize) -> &str {
        self.directory()[i].key.as_str()
    }

    /// The checksum at positional index `i` (directory order), no blob access.
    pub fn checksum_at(&self, i: usize) -> u64 {
        self.directory()[i].checksum.to_native()
    }

    pub fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        self.directory().iter().map(move |d| {
            self.blob(d.offset.to_native(), d.len.to_native())
                .expect("corrupt document blob during iteration")
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.directory().iter().map(|d| d.key.as_str())
    }
}

impl Index<usize> for MmapArchive {
    type Output = ArchivedHtmlEntry;

    /// Positional access (directory order). Panics on a corrupt blob — `Index` cannot return a
    /// `Result`, and a bad blob at a valid index means the archive is corrupt.
    fn index(&self, index: usize) -> &Self::Output {
        let d = &self.directory()[index];
        self.blob(d.offset.to_native(), d.len.to_native())
            .expect("corrupt document blob")
    }
}

impl crate::Archive for MmapArchive {
    type Entry = ArchivedHtmlEntry;

    fn len(&self) -> usize {
        self.directory().len()
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
