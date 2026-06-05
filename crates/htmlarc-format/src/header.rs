//! The `.htmlarc` file header.
//!
//! Layout (16 bytes — a multiple of 8 so the rkyv payload that follows keeps its
//! 8-byte root alignment when accessed at `&bytes[HEADER_LEN..]`):
//!
//! | bytes  | meaning                              |
//! |--------|--------------------------------------|
//! | 0..8   | magic `b"HTMLARC1"`                  |
//! | 8      | format version                       |
//! | 9      | endianness (0 = little-endian)       |
//! | 10..16 | reserved (zero)                      |
//!
//! Files written before the header existed have no magic; they are detected as
//! "legacy" (payload at offset 0) so old archives keep loading.

use crate::error::ArchiveErr;

pub(crate) const MAGIC: &[u8; 8] = b"HTMLARC1";
pub(crate) const VERSION: u8 = 1;
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

/// Validate the header (if present) and return the byte offset at which the rkyv
/// payload begins: `HEADER_LEN` for a modern file, `0` for a legacy (header-less)
/// one. Errors on a recognized-but-unsupported header (wrong version/endianness).
pub(crate) fn payload_offset(bytes: &[u8]) -> Result<usize, ArchiveErr> {
    if bytes.len() >= HEADER_LEN && &bytes[0..8] == MAGIC {
        let version = bytes[8];
        if version != VERSION {
            return Err(ArchiveErr::Header(format!(
                "unsupported .htmlarc version {version} (this build supports {VERSION})"
            )));
        }
        let endian = bytes[9];
        if endian != ENDIAN_LITTLE {
            return Err(ArchiveErr::Header(format!(
                "unsupported endianness byte {endian} (only little-endian is supported)"
            )));
        }
        Ok(HEADER_LEN)
    } else {
        // No magic: a legacy, header-less archive — the whole file is the payload.
        Ok(0)
    }
}
