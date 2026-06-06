//! The fixed-size **trailer** at the end of a v3 `.htmlarc`, so a reader can bootstrap the
//! whole file by reading just the last [`TRAILER_LEN`] bytes.
//!
//! Layout (48 bytes, hand-rolled little-endian like the header — *not* rkyv, so it has no
//! alignment requirement and is read straight off the tail):
//!
//! | bytes  | meaning                                   |
//! |--------|-------------------------------------------|
//! | 0..8   | directory blob offset                     |
//! | 8..16  | directory blob length (exact, unpadded)   |
//! | 16..24 | shared-dict region offset                 |
//! | 24..32 | shared-dict region length (0 for now)     |
//! | 32..40 | document count                            |
//! | 40..48 | magic `b"HARCFOOT"`                       |

use crate::error::ArchiveErr;
use crate::header::HEADER_LEN;

pub(crate) const TRAILER_MAGIC: &[u8; 8] = b"HARCFOOT";
pub(crate) const TRAILER_LEN: usize = 48;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Trailer {
    pub dir_offset: u64,
    pub dir_len: u64,
    pub dict_offset: u64,
    pub dict_len: u64,
    pub doc_count: u64,
}

impl Trailer {
    pub(crate) fn to_bytes(self) -> [u8; TRAILER_LEN] {
        let mut b = [0u8; TRAILER_LEN];
        b[0..8].copy_from_slice(&self.dir_offset.to_le_bytes());
        b[8..16].copy_from_slice(&self.dir_len.to_le_bytes());
        b[16..24].copy_from_slice(&self.dict_offset.to_le_bytes());
        b[24..32].copy_from_slice(&self.dict_len.to_le_bytes());
        b[32..40].copy_from_slice(&self.doc_count.to_le_bytes());
        b[40..48].copy_from_slice(TRAILER_MAGIC);
        b
    }

    /// Read and validate the trailer from the tail of a whole-file byte slice. Bounds-checks
    /// the directory range so a corrupt/truncated file becomes an `Err`, never a panic.
    pub(crate) fn read_from_tail(file: &[u8]) -> Result<Trailer, ArchiveErr> {
        if file.len() < HEADER_LEN + TRAILER_LEN {
            return Err(ArchiveErr::Validate(
                "file too small to contain a v3 trailer".into(),
            ));
        }
        let tail = &file[file.len() - TRAILER_LEN..];
        if &tail[40..48] != TRAILER_MAGIC {
            return Err(ArchiveErr::Validate(
                "missing .htmlarc footer magic (truncated or not a v3 archive)".into(),
            ));
        }
        let rd = |r: std::ops::Range<usize>| {
            let mut a = [0u8; 8];
            a.copy_from_slice(&tail[r]);
            u64::from_le_bytes(a)
        };
        let t = Trailer {
            dir_offset: rd(0..8),
            dir_len: rd(8..16),
            dict_offset: rd(16..24),
            dict_len: rd(24..32),
            doc_count: rd(32..40),
        };

        // The directory must live in the data region, before the trailer.
        let footer_start = (file.len() - TRAILER_LEN) as u64;
        let dir_end = t
            .dir_offset
            .checked_add(t.dir_len)
            .ok_or_else(|| ArchiveErr::Validate("directory offset/len overflow".into()))?;
        if t.dir_offset < HEADER_LEN as u64 || dir_end > footer_start {
            return Err(ArchiveErr::Validate(
                "directory range lies outside the file".into(),
            ));
        }
        Ok(t)
    }
}
