//! The `.htmlarc` file header.
//!
//! Layout (16 bytes — a multiple of 8 so the first rkyv doc blob that follows keeps its
//! 8-byte alignment when accessed at `&bytes[HEADER_LEN..]`):
//!
//! | bytes  | meaning                                              |
//! |--------|------------------------------------------------------|
//! | 0..8   | magic `b"HTMLARC1"`                                  |
//! | 8      | format version (12 = metadata columns)               |
//! | 9      | endianness (0 = little-endian)                       |
//! | 10..16 | u48 LE: last-good trailer offset while an in-place   |
//! |        | append is in flight (ADR 0010); 0 otherwise          |
//!
//! Version 12 adds an optional typed per-document metadata table (ADR 0009): a columnar
//! rkyv blob in the footer region located via the trailer's `meta_offset`/`meta_len`
//! (the trailer grew 88 → 104 bytes), one row per document in arrival order. Otherwise
//! the layout is v11's: a bundle-segmented, footer-indexed container (see [`crate::trailer`],
//! [`crate::doc_table`], [`crate::bundle`]) whose per-document DOM unifies standard, `data-*`,
//! and unknown attributes into one attribute store (ADR 0002 §3) and stores extended
//! (custom/unknown) tag names in a per-document vocab encoded in the node tag byte (ADR 0002
//! §4). Each document's text/comment pool is relocated out of its blob into its bundle's
//! [`BundleStrings`](crate::bundle_strings) block (ADR 0006) and stored as ~16 KiB zstd blocks
//! cut at text-node boundaries (ADR 0008; v10 stored one frame per document, v9 stored the
//! block uncompressed), optionally against one archive-wide dictionary recorded in the trailer
//! (ADR 0005). v11 and older layouts are no longer read — re-pack to upgrade.

use crate::error::ArchiveErr;

pub(crate) const MAGIC: &[u8; 8] = b"HTMLARC1";
pub(crate) const VERSION: u8 = 12;
pub(crate) const ENDIAN_LITTLE: u8 = 0;
pub(crate) const HEADER_LEN: usize = 16;

/// The 16-byte header prepended to every freshly written `.htmlarc`.
pub(crate) fn header_bytes() -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[0..8].copy_from_slice(MAGIC);
    header[8] = VERSION;
    header[9] = ENDIAN_LITTLE;
    header
}

/// Validate the front header. Errors on a missing magic (legacy/header-less or not a
/// `.htmlarc`) or a recognized-but-unsupported version/endianness — old archives must be
/// re-packed with this build.
pub(crate) fn validate_header(bytes: &[u8]) -> Result<(), ArchiveErr> {
    if bytes.len() < HEADER_LEN || &bytes[0..8] != MAGIC {
        return Err(ArchiveErr::Header(
            "not a .htmlarc file (missing magic — legacy archives must be re-packed)".into(),
        ));
    }
    let version = bytes[8];
    if version != VERSION {
        return Err(ArchiveErr::Header(format!(
            "unsupported .htmlarc version {version} (this build reads/writes {VERSION}; re-pack to upgrade)"
        )));
    }
    let endian = bytes[9];
    if endian != ENDIAN_LITTLE {
        return Err(ArchiveErr::Header(format!(
            "unsupported endianness byte {endian} (only little-endian is supported)"
        )));
    }
    Ok(())
}

/// The byte offset within the header where the append-recovery offset lives.
pub(crate) const PENDING_TRAILER_AT: usize = 10;

/// The staged last-good trailer offset (header bytes `10..16`, u48 LE), set for the duration
/// of an in-place append (ADR 0010). `None` when zero — no append in flight, the tail trailer
/// is authoritative.
pub(crate) fn pending_trailer_offset(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < HEADER_LEN {
        return None;
    }
    let mut a = [0u8; 8];
    a[..6].copy_from_slice(&bytes[PENDING_TRAILER_AT..HEADER_LEN]);
    let off = u64::from_le_bytes(a);
    (off != 0).then_some(off)
}

/// The 6-byte u48 LE encoding of `offset` for header bytes `10..16`. Errors only past 256 TiB.
pub(crate) fn pending_trailer_bytes(offset: u64) -> Result<[u8; 6], ArchiveErr> {
    if offset >= 1 << 48 {
        return Err(ArchiveErr::Validate(
            "archive too large for an in-place append recovery offset".into(),
        ));
    }
    let mut b = [0u8; 6];
    b.copy_from_slice(&offset.to_le_bytes()[..6]);
    Ok(b)
}
