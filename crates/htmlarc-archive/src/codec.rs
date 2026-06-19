//! The per-document string codec (ADR 0005).
//!
//! Each document's relocated text/comment pool ([`BundleStrings`](crate::bundle_strings)) is
//! compressed as one independent zstd frame at [`COMPRESSION_LEVEL`], optionally against one
//! archive-wide dictionary recorded in the [trailer](crate::trailer). Compression happens at write
//! time; the read path inflates lazily, one document at a time, through the [`FrameDecoder`] this
//! module installs on an opened archive — so a query that never touches a document's text never
//! pays to inflate it.
//!
//! The frame is keyed by the document's exact decompressed length (stored alongside it), so a
//! decode allocates its output buffer once, with no over-read. An empty pool is stored as a
//! zero-length frame and skips the codec entirely on both sides.

use htmlarc_dom::prelude::FrameDecoder;
use zstd::bulk::{Compressor, Decompressor};
use zstd::dict::{DecoderDictionary, EncoderDictionary};

use crate::error::ArchiveErr;

/// zstd level for the per-document frames. A build-time knob only: decoding is level-independent,
/// so this can be raised later without touching already-written archives (ADR 0005).
pub(crate) const COMPRESSION_LEVEL: i32 = 3;

/// Trained-dictionary size cap (110 KiB). The harness study found the ratio plateaus well before a
/// dictionary this large, and ~500 sample documents already saturate it (ADR 0005).
pub(crate) const DICT_MAX: usize = 112_640;

/// Minimum sample text before training is worthwhile. A trained dictionary is stored in the archive
/// (up to [`DICT_MAX`]); below a few MiB of sample text it cannot reliably amortize that stored
/// size, and the harness study found the ratio only nears its plateau around several hundred
/// documents. Toy inputs therefore stay dictionary-less rather than carry a dictionary that does
/// not pay for itself (ADR 0005).
pub(crate) const DICT_MIN_SAMPLE_BYTES: usize = 4 << 20;

/// Train one archive-wide dictionary from a sample of raw document text pools. Returns `None` when
/// there is too little to train on — zstd needs a handful of non-trivial samples, and below
/// [`DICT_MIN_SAMPLE_BYTES`] the stored dictionary would not pay for itself — in which case the
/// caller compresses dictionary-less, still a valid archive.
pub fn train_string_dict<S: AsRef<[u8]>>(samples: &[S]) -> Option<Vec<u8>> {
    let usable = samples.iter().filter(|s| !s.as_ref().is_empty()).count();
    let total: usize = samples.iter().map(|s| s.as_ref().len()).sum();
    if usable < 8 || total < DICT_MIN_SAMPLE_BYTES {
        return None;
    }
    match zstd::dict::from_samples(samples, DICT_MAX) {
        Ok(dict) if !dict.is_empty() => Some(dict),
        _ => None,
    }
}

/// A reusable compressor for one writer/worker. Dictionary-less.
pub(crate) fn build_compressor() -> Result<Compressor<'static>, ArchiveErr> {
    Compressor::new(COMPRESSION_LEVEL).map_err(|e| ArchiveErr::Serialize(e.to_string()))
}

/// The shared, immutable string-compression context for one archive: a prepared `CDict` (or none,
/// for dictionary-less). Built once and shared across convert workers (it is `Sync`); each worker
/// makes its own short-lived [`StringCompressor`] from it, so the dictionary is digested once, not
/// per document or per thread. The matching raw dictionary bytes are stored in the archive trailer
/// so the reader can rebuild the decoder (ADR 0005).
pub struct StringEncoder {
    cdict: Option<EncoderDictionary<'static>>,
}

impl StringEncoder {
    /// Build from the trained dictionary bytes (or `None` for dictionary-less compression).
    pub fn new(dict: Option<&[u8]>) -> Self {
        Self {
            cdict: dict.map(|d| EncoderDictionary::copy(d, COMPRESSION_LEVEL)),
        }
    }

    /// A per-worker compressor bound to this encoder's dictionary (reusing the prepared `CDict`).
    pub fn compressor(&self) -> Result<StringCompressor<'_>, ArchiveErr> {
        let inner = match &self.cdict {
            Some(cd) => Compressor::with_prepared_dictionary(cd)
                .map_err(|e| ArchiveErr::Serialize(e.to_string()))?,
            None => build_compressor()?,
        };
        Ok(StringCompressor { inner })
    }
}

/// One worker/thread's compressor over a [`StringEncoder`]'s dictionary. Not `Sync` (it carries a
/// mutable zstd context) — make one per worker from the shared encoder.
pub struct StringCompressor<'a> {
    inner: Compressor<'a>,
}

impl StringCompressor<'_> {
    /// Compress one document's raw text pool into its frame (empty stays empty).
    pub fn compress(&mut self, raw: &[u8]) -> Result<Vec<u8>, ArchiveErr> {
        compress_segment(&mut self.inner, raw)
    }
}

/// Compress one document's raw text pool into a standalone frame, reusing `compressor`'s context.
/// An empty pool stays empty (a zero-length frame): a text-free document costs nothing to store and
/// needs no inflate on read.
pub(crate) fn compress_segment(
    compressor: &mut Compressor<'_>,
    raw: &[u8],
) -> Result<Vec<u8>, ArchiveErr> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    compressor
        .compress(raw)
        .map_err(|e| ArchiveErr::Serialize(e.to_string()))
}

/// The [`FrameDecoder`] installed on an opened archive: inflates one per-document frame, reusing
/// the archive-wide dictionary when present. Holds the *prepared* (digested) `DDict` — built once
/// at open, never per call — and it is immutable, so the decoder is `Sync` and a single instance
/// can be borrowed by every reader thread; each decode allocates only its own decompression
/// context, which is the thread-safe grain.
pub(crate) struct ZstdFrameDecoder {
    /// The archive-wide dictionary (digested), or `None` when the strings were compressed
    /// dictionary-less.
    ddict: Option<DecoderDictionary<'static>>,
}

impl ZstdFrameDecoder {
    pub(crate) fn new(dict: Option<Vec<u8>>) -> Self {
        Self {
            ddict: dict.map(|d| DecoderDictionary::copy(&d)),
        }
    }
}

impl FrameDecoder for ZstdFrameDecoder {
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn decode(&self, frame: &[u8], raw_len: usize) -> Vec<u8> {
        // A zero-length frame is a text-free document — never a real zstd frame.
        if frame.is_empty() {
            debug_assert_eq!(raw_len, 0, "empty frame must decode to nothing");
            return Vec::new();
        }
        // A frame that fails to inflate means the archive is corrupt; like a bad document blob,
        // that is a panic (the read API cannot return a `Result` from a text accessor).
        let out = match &self.ddict {
            None => zstd::bulk::decompress(frame, raw_len),
            Some(ddict) => Decompressor::with_prepared_dictionary(ddict)
                .and_then(|mut d| d.decompress(frame, raw_len)),
        };
        out.expect("corrupt string frame")
    }
}
