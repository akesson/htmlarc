//! The fixed-size **trailer** at the end of a `.htmlarc`, so a reader can bootstrap the
//! whole file by reading just the last [`TRAILER_LEN`] bytes.
//!
//! Layout (104 bytes, hand-rolled little-endian like the header — *not* rkyv, so it has no
//! alignment requirement and is read straight off the tail):
//!
//! | bytes   | meaning                                   |
//! |---------|-------------------------------------------|
//! | 0..8    | doc-table blob offset                     |
//! | 8..16   | doc-table blob length (exact, unpadded)   |
//! | 16..24  | bundle-table blob offset                  |
//! | 24..32  | bundle-table blob length                  |
//! | 32..40  | sort-index blob offset                    |
//! | 40..48  | sort-index blob length                    |
//! | 48..56  | dictionary-region offset                  |
//! | 56..64  | dictionary-region length (0 = no dict)    |
//! | 64..72  | metadata-table blob offset                |
//! | 72..80  | metadata-table blob length (0 = no meta)  |
//! | 80..88  | document count                            |
//! | 88..96  | bundle count                              |
//! | 96..104 | magic `b"HARCFOOT"`                       |
//!
//! The per-bundle string blocks are interleaved with the document blobs and located via the
//! bundle table (each [`BundleDesc`](crate::bundle::BundleDesc) carries its own offset/length).
//! The old single contiguous "data region" slot now holds the archive-wide string-compression
//! dictionary (ADR 0005); `dict_len == 0` means the strings were compressed dictionary-less.

use crate::error::ArchiveErr;
use crate::header::HEADER_LEN;

pub(crate) const TRAILER_MAGIC: &[u8; 8] = b"HARCFOOT";
pub(crate) const TRAILER_LEN: usize = 104;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Trailer {
    pub doc_table_offset: u64,
    pub doc_table_len: u64,
    pub bundle_table_offset: u64,
    pub bundle_table_len: u64,
    pub sort_index_offset: u64,
    pub sort_index_len: u64,
    pub dict_offset: u64,
    pub dict_len: u64,
    pub meta_offset: u64,
    pub meta_len: u64,
    pub doc_count: u64,
    pub bundle_count: u64,
}

impl Trailer {
    pub(crate) fn to_bytes(self) -> [u8; TRAILER_LEN] {
        let mut b = [0u8; TRAILER_LEN];
        b[0..8].copy_from_slice(&self.doc_table_offset.to_le_bytes());
        b[8..16].copy_from_slice(&self.doc_table_len.to_le_bytes());
        b[16..24].copy_from_slice(&self.bundle_table_offset.to_le_bytes());
        b[24..32].copy_from_slice(&self.bundle_table_len.to_le_bytes());
        b[32..40].copy_from_slice(&self.sort_index_offset.to_le_bytes());
        b[40..48].copy_from_slice(&self.sort_index_len.to_le_bytes());
        b[48..56].copy_from_slice(&self.dict_offset.to_le_bytes());
        b[56..64].copy_from_slice(&self.dict_len.to_le_bytes());
        b[64..72].copy_from_slice(&self.meta_offset.to_le_bytes());
        b[72..80].copy_from_slice(&self.meta_len.to_le_bytes());
        b[80..88].copy_from_slice(&self.doc_count.to_le_bytes());
        b[88..96].copy_from_slice(&self.bundle_count.to_le_bytes());
        b[96..104].copy_from_slice(TRAILER_MAGIC);
        b
    }

    /// Read and validate the trailer from the tail of a whole-file byte slice, falling back to
    /// the header's staged recovery offset (ADR 0010) when the tail is not a valid trailer —
    /// which is exactly the state a crashed or in-progress in-place append leaves behind. The
    /// recovered trailer describes the pre-append archive; bytes past it are ignored garbage.
    pub(crate) fn read_from_tail(file: &[u8]) -> Result<Trailer, ArchiveErr> {
        if file.len() < HEADER_LEN + TRAILER_LEN {
            return Err(ArchiveErr::Validate(
                "file too small to contain a v4 trailer".into(),
            ));
        }
        match Self::read_at(file, file.len() - TRAILER_LEN) {
            Ok(t) => Ok(t),
            Err(e) => match crate::header::pending_trailer_offset(file) {
                Some(off) => Self::read_at(file, off as usize).map_err(|_| e),
                None => Err(e),
            },
        }
    }

    /// Read and validate the trailer at `trailer_offset`. Bounds-checks every footer region
    /// against that offset so a corrupt/truncated file becomes an `Err`, never a panic.
    pub(crate) fn read_at(file: &[u8], trailer_offset: usize) -> Result<Trailer, ArchiveErr> {
        if trailer_offset < HEADER_LEN
            || trailer_offset
                .checked_add(TRAILER_LEN)
                .is_none_or(|end| end > file.len())
        {
            return Err(ArchiveErr::Validate(
                "trailer offset lies outside the file".into(),
            ));
        }
        let tail = &file[trailer_offset..trailer_offset + TRAILER_LEN];
        if &tail[96..104] != TRAILER_MAGIC {
            return Err(ArchiveErr::Validate(
                "missing .htmlarc footer magic (truncated or not a v4 archive)".into(),
            ));
        }
        let rd = |r: std::ops::Range<usize>| {
            let mut a = [0u8; 8];
            a.copy_from_slice(&tail[r]);
            u64::from_le_bytes(a)
        };
        let t = Trailer {
            doc_table_offset: rd(0..8),
            doc_table_len: rd(8..16),
            bundle_table_offset: rd(16..24),
            bundle_table_len: rd(24..32),
            sort_index_offset: rd(32..40),
            sort_index_len: rd(40..48),
            dict_offset: rd(48..56),
            dict_len: rd(56..64),
            meta_offset: rd(64..72),
            meta_len: rd(72..80),
            doc_count: rd(80..88),
            bundle_count: rd(88..96),
        };

        // Every footer region must live in the data area, between the header and the trailer.
        let footer_start = trailer_offset as u64;
        for (off, len, what) in [
            (t.doc_table_offset, t.doc_table_len, "doc table"),
            (t.bundle_table_offset, t.bundle_table_len, "bundle table"),
            (t.sort_index_offset, t.sort_index_len, "sort index"),
            (t.dict_offset, t.dict_len, "dictionary region"),
            (t.meta_offset, t.meta_len, "metadata table"),
        ] {
            let end = off
                .checked_add(len)
                .ok_or_else(|| ArchiveErr::Validate(format!("{what} offset/len overflow")))?;
            if off < HEADER_LEN as u64 || end > footer_start {
                return Err(ArchiveErr::Validate(format!(
                    "{what} range lies outside the file"
                )));
            }
        }
        Ok(t)
    }
}
