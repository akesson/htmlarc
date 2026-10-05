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

use std::cell::RefCell;

use htmlarc_dom::prelude::FrameDecoder;
use zstd::bulk::Compressor;
use zstd::dict::{DecoderDictionary, EncoderDictionary};
use zstd::zstd_safe::{DCtx, SafeResult, get_error_name};

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
/// `DICT_MIN_SAMPLE_BYTES` the stored dictionary would not pay for itself — in which case the
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
    /// Compress one document's raw pool as independent block frames (cut at `raw_ends`, from
    /// `crate::bundle_strings::block_cuts`): the concatenated frames plus their cumulative ends.
    pub fn compress_pool(
        &mut self,
        raw: &[u8],
        raw_ends: &[u32],
    ) -> Result<(Vec<u8>, Vec<u32>), ArchiveErr> {
        compress_pool_blocks(&mut self.inner, raw, raw_ends)
    }
}

/// Compress one document's pool block by block: each `raw[prev_end..end]` slice becomes its own
/// standalone frame (blocks are never empty — [`block_cuts`](crate::bundle_strings::block_cuts)
/// only emits strictly increasing ends), concatenated with cumulative frame-end offsets to match.
pub(crate) fn compress_pool_blocks(
    compressor: &mut Compressor<'_>,
    raw: &[u8],
    raw_ends: &[u32],
) -> Result<(Vec<u8>, Vec<u32>), ArchiveErr> {
    let mut frames = Vec::new();
    let mut frame_ends = Vec::with_capacity(raw_ends.len());
    let mut start = 0usize;
    for &end in raw_ends {
        let frame = compress_segment(compressor, &raw[start..end as usize])?;
        frames.extend_from_slice(&frame);
        frame_ends.push(frames.len() as u32);
        start = end as usize;
    }
    Ok((frames, frame_ends))
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
/// can be borrowed by every reader thread. The decompression context is per thread and reused,
/// never made per frame: a parallel sweep's create/free pairs contend on the system allocator.
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

thread_local! {
    /// The decoding thread's reusable decompression context. It belongs to the thread, not to an
    /// archive: every decode passes its own dictionary (or none), so one context serves them all.
    static DCTX: RefCell<DCtx<'static>> = RefCell::new(DCtx::create());
}

impl ZstdFrameDecoder {
    /// Inflate `frame` into `out` on `dctx`, against this archive's dictionary when it has one.
    fn inflate(&self, dctx: &mut DCtx<'_>, out: &mut Vec<u8>, frame: &[u8]) -> SafeResult {
        match &self.ddict {
            None => dctx.decompress(out, frame),
            Some(ddict) => dctx.decompress_using_ddict(out, frame, ddict.as_ddict()),
        }
    }
}

impl FrameDecoder for ZstdFrameDecoder {
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn decode(&self, frame: &[u8], raw_len: usize) -> Vec<u8> {
        // A frame that fails to inflate means the archive is corrupt; like a bad document blob,
        // that is a panic (the read API cannot return a `Result` from a text accessor).
        // A zero-length frame is a text-free document — never a real zstd frame.
        if frame.is_empty() {
            assert_eq!(
                raw_len, 0,
                "corrupt string frame: empty frame for {raw_len} bytes"
            );
            return Vec::new();
        }
        let mut out = Vec::with_capacity(raw_len);
        // A thread whose locals are already torn down (a decode from a TLS destructor) gets a
        // one-off context instead.
        DCTX.try_with(|dctx| self.inflate(&mut dctx.borrow_mut(), &mut out, frame))
            .unwrap_or_else(|_| self.inflate(&mut DCtx::create(), &mut out, frame))
            .unwrap_or_else(|code| panic!("corrupt string frame: {}", get_error_name(code)));
        assert_eq!(
            out.len(),
            raw_len,
            "corrupt string frame: wrong decoded length"
        );
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text pools big and varied enough to clear `train_string_dict`'s thresholds, so the
    /// dictionary path runs; the test archives are all below them and stay dictionary-less.
    fn samples() -> Vec<Vec<u8>> {
        (0..3_000u32)
            .map(|i| {
                (0..120u32)
                    .map(|w| format!("word{} item-{} ", (i * 7 + w * 13) % 97, w % 11))
                    .collect::<String>()
                    .into_bytes()
            })
            .collect()
    }

    fn compress(encoder: &StringEncoder, raw: &[u8]) -> Vec<u8> {
        let (frames, ends) = encoder
            .compressor()
            .unwrap()
            .compress_pool(raw, &[raw.len() as u32])
            .unwrap();
        assert_eq!(ends, [frames.len() as u32]);
        frames
    }

    #[test]
    fn decoders_with_and_without_dictionary_share_a_thread() {
        let samples = samples();
        let dict = train_string_dict(&samples).expect("samples clear the training threshold");
        let (with, without) = (StringEncoder::new(Some(&dict)), StringEncoder::new(None));
        let (dec_with, dec_without) = (
            ZstdFrameDecoder::new(Some(dict)),
            ZstdFrameDecoder::new(None),
        );
        // Alternate on one thread, so both reuse the same thread-local context back to back.
        for raw in samples.iter().step_by(97) {
            assert_eq!(dec_with.decode(&compress(&with, raw), raw.len()), *raw);
            assert_eq!(
                dec_without.decode(&compress(&without, raw), raw.len()),
                *raw
            );
        }
        assert!(dec_with.decode(&[], 0).is_empty());
    }

    #[test]
    fn corrupt_frame_panics_with_the_zstd_error_name_and_the_thread_recovers() {
        let samples = samples();
        let dict = train_string_dict(&samples).unwrap();
        let raw = &samples[0];
        let frame = compress(&StringEncoder::new(Some(&dict)), raw);
        // A dictionary frame read without its dictionary.
        let err =
            std::panic::catch_unwind(|| ZstdFrameDecoder::new(None).decode(&frame, raw.len()))
                .expect_err("a dictionary frame must not decode without the dictionary");
        let msg = err.downcast_ref::<String>().unwrap();
        assert!(msg.starts_with("corrupt string frame: "), "{msg}");
        assert!(
            msg.chars().any(char::is_alphabetic),
            "names the zstd error: {msg}"
        );
        // The panic left the thread's context usable.
        assert_eq!(
            ZstdFrameDecoder::new(Some(dict)).decode(&frame, raw.len()),
            *raw
        );
    }
}
