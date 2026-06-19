//! Per-bundle document-string store.
//!
//! Each document's text/comment pool used to live inline in its own rkyv blob. Relocating every
//! pool in a bundle into one [`BundleStrings`] block (written into the bundle's reserved data
//! region, see [`crate::writer`]) is what lets a whole bundle share one storage decision: each
//! pool is compressed into its own independent zstd frame (ADR 0005, [`crate::codec`]), so a
//! reader borrows the *compressed* frame zero-copy and inflates exactly the documents it reads,
//! lazily, through [`StringSource::Lazy`].

use std::cell::OnceCell;

use htmlarc_dom::prelude::{FrameDecoder, LazyState};
use rkyv::{Archive, Deserialize, Serialize};

/// The text/comment pools of every document in a bundle, each compressed into its own zstd frame
/// and concatenated into one byte block with two per-document offset tables. Both tables have
/// `doc_count + 1` cumulative entries: document `slot` owns the frame
/// `frames[frame_offsets[slot]..frame_offsets[slot + 1]]`, which inflates to a segment of
/// `raw_offsets[slot + 1] - raw_offsets[slot]` bytes (its node text ranges — document-local —
/// index into that inflated segment). A text-free document is a zero-length frame.
#[derive(Archive, Serialize, Deserialize, Clone)]
pub struct BundleStrings {
    frames: Vec<u8>,
    frame_offsets: Vec<u32>,
    raw_offsets: Vec<u32>,
}

/// An empty store with valid (sentinel-`0`) offset tables, so `push_doc_frame` and
/// `std::mem::take` are always sound — never bare `Vec::new()`s that would underflow `doc_count`.
impl Default for BundleStrings {
    fn default() -> Self {
        Self::with_doc_capacity(0)
    }
}

impl BundleStrings {
    /// An empty store sized for `docs` documents.
    pub fn with_doc_capacity(docs: usize) -> Self {
        let mut frame_offsets = Vec::with_capacity(docs + 1);
        frame_offsets.push(0);
        let mut raw_offsets = Vec::with_capacity(docs + 1);
        raw_offsets.push(0);
        Self {
            frames: Vec::new(),
            frame_offsets,
            raw_offsets,
        }
    }

    /// Append one document's compressed `frame` (which inflates to `raw_len` bytes), returning its
    /// slot. The caller has already run the segment through [`crate::codec::compress_segment`].
    pub fn push_doc_frame(&mut self, frame: &[u8], raw_len: u32) -> usize {
        let slot = self.frame_offsets.len() - 1;
        self.frames.extend_from_slice(frame);
        self.frame_offsets.push(self.frames.len() as u32);
        let prev_raw = *self
            .raw_offsets
            .last()
            .expect("sentinel keeps this non-empty");
        self.raw_offsets.push(prev_raw + raw_len);
        slot
    }

    pub fn doc_count(&self) -> usize {
        self.frame_offsets.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.doc_count() == 0
    }

    /// Total stored (compressed) byte length — the concatenated frames.
    pub fn byte_len(&self) -> usize {
        self.frames.len()
    }
}

impl ArchivedBundleStrings {
    /// The number of documents this block covers.
    pub fn doc_count(&self) -> usize {
        self.frame_offsets.len() - 1
    }

    /// Document `slot`'s compressed frame.
    pub fn frame(&self, slot: usize) -> &[u8] {
        let start = self.frame_offsets[slot].to_native() as usize;
        let end = self.frame_offsets[slot + 1].to_native() as usize;
        &self.frames[start..end]
    }

    /// Document `slot`'s decompressed (raw) length — the inflated segment size, and the exact
    /// buffer capacity its frame decodes into.
    pub fn raw_len(&self, slot: usize) -> u32 {
        self.raw_offsets[slot + 1].to_native() - self.raw_offsets[slot].to_native()
    }

    /// Build a per-document [`LazyState`] for every slot, each pairing this block's frame with a
    /// caller-owned inflate cache (`bufs[slot]`) and the archive's `decoder`. The caller owns
    /// `bufs` and the returned `Vec` as two separate values in its read scope, so nothing here is
    /// self-referential; a bound document inflates its frame at most once, only when its text is
    /// actually read. `bufs.len()` must be `doc_count()`.
    pub fn lazy_states<'s>(
        &'s self,
        decoder: &'s dyn FrameDecoder,
        bufs: &'s [OnceCell<Vec<u8>>],
    ) -> Vec<LazyState<'s>> {
        debug_assert_eq!(
            bufs.len(),
            self.doc_count(),
            "one inflate cache per document"
        );
        (0..self.doc_count())
            .map(|slot| LazyState {
                buf: &bufs[slot],
                frame: self.frame(slot),
                base: 0,
                len: self.raw_len(slot),
                decoder,
            })
            .collect()
    }

    /// Validate the offset tables against the frame block: both cumulative, monotonic, starting at
    /// `0`, with equal lengths (one entry per document plus a sentinel), and the frame table ending
    /// exactly at the block length. A corrupt table would otherwise panic (or slice out of range)
    /// at read time; surfacing it here keeps `MmapArchive::open` the single validation gate. (Frame
    /// *contents* are checked lazily — a bad frame panics on inflate, like a bad document blob.)
    pub fn validate(&self) -> bool {
        if self.frame_offsets.len() != self.raw_offsets.len() {
            return false;
        }
        if self.frame_offsets.is_empty()
            || self.frame_offsets[0].to_native() != 0
            || self.raw_offsets[0].to_native() != 0
        {
            return false;
        }
        let frame_mono = monotonic(self.frame_offsets.iter().map(|o| o.to_native()));
        let raw_mono = monotonic(self.raw_offsets.iter().map(|o| o.to_native()));
        if !frame_mono || !raw_mono {
            return false;
        }
        self.frame_offsets[self.frame_offsets.len() - 1].to_native() as usize == self.frames.len()
    }
}

/// A cumulative offset table is monotonic non-decreasing.
fn monotonic(offsets: impl Iterator<Item = u32>) -> bool {
    let mut prev = 0u32;
    for v in offsets {
        if v < prev {
            return false;
        }
        prev = v;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{ZstdFrameDecoder, build_compressor, compress_segment};
    use rkyv::rancor::Error;

    /// Round-trip three documents (including an empty middle one) through compression + the
    /// archived offset tables + the decoder, recovering each segment exactly.
    #[test]
    fn round_trip_segments_across_docs() {
        let docs: [&[u8]; 3] = [b"alpha alpha alpha", b"", b"gamma! gamma! gamma!"];
        let mut c = build_compressor().unwrap();

        let mut bs = BundleStrings::with_doc_capacity(3);
        for (slot, raw) in docs.iter().enumerate() {
            let frame = compress_segment(&mut c, raw).unwrap();
            assert_eq!(bs.push_doc_frame(&frame, raw.len() as u32), slot);
        }
        assert_eq!(bs.doc_count(), 3);

        let bytes = rkyv::to_bytes::<Error>(&bs).unwrap();
        let arch = rkyv::access::<ArchivedBundleStrings, Error>(&bytes).unwrap();
        assert!(arch.validate());
        assert_eq!(arch.doc_count(), 3);

        let decoder = ZstdFrameDecoder::new(None);
        for (slot, raw) in docs.iter().enumerate() {
            assert_eq!(arch.raw_len(slot) as usize, raw.len());
            assert_eq!(
                decoder.decode(arch.frame(slot), arch.raw_len(slot) as usize),
                *raw
            );
        }
        // The empty middle document is a zero-length frame.
        assert!(arch.frame(1).is_empty());
    }
}
