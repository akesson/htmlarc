//! The `.htmlarc` file header.
//!
//! Layout (16 bytes — a multiple of 8 so the first rkyv doc blob that follows keeps its
//! 8-byte alignment when accessed at `&bytes[HEADER_LEN..]`):
//!
//! | bytes  | meaning                                |
//! |--------|----------------------------------------|
//! | 0..8   | magic `b"HTMLARC1"`                    |
//! | 8      | format version (4 = bundle-segmented)  |
//! | 9      | endianness (0 = little-endian)         |
//! | 10..16 | reserved (zero)                        |
//!
//! Version 4 is a bundle-segmented, footer-indexed container (see [`crate::trailer`],
//! [`crate::doc_table`], [`crate::bundle`]); the older flat v3 and legacy layouts are no longer
//! read — re-pack to upgrade.

use crate::error::ArchiveErr;

pub(crate) const MAGIC: &[u8; 8] = b"HTMLARC1";
pub(crate) const VERSION: u8 = 4;
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
