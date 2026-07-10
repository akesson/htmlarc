//! Per-bundle document-string store.
//!
//! Each document's text/comment pool used to live inline in its own rkyv blob. Relocating every
//! pool in a bundle into one [`BundleStrings`] block (written into the bundle's reserved data
//! region, see [`crate::writer`]) is what lets a whole bundle share one storage decision: each
//! pool is split into ~[`TEXT_BLOCK_SIZE`] blocks cut at text-node boundaries, each block
//! compressed into its own independent zstd frame (ADR 0005 + ADR 0008, [`crate::codec`]), so a
//! reader borrows the *compressed* frames zero-copy and inflates exactly the blocks it reads,
//! lazily, through [`StringSource::Lazy`](htmlarc_dom::prelude::StringSource). A sweep that
//! touches a sliver of a large document's text (a headings query, say) inflates one block, not
//! the whole pool.

use std::ops::Range;
use std::sync::OnceLock;

use htmlarc_dom::prelude::{FrameDecoder, LazyState};
use rkyv::{Archive, Deserialize, Serialize};

/// Target (not maximum) inflated size of one compressed text block. Blocks are cut only at
/// text-node boundaries — a single node larger than this yields one oversized block — so a
/// node's range never straddles blocks and reads stay borrowed slices. 16 KiB is the measured
/// knee: +14% on the stored string block (≈ +2% archive) for ~3× less inflate work on selective
/// sweeps; 8 KiB pays +23% for little further gain (ADR 0008). Most documents' pools are smaller
/// than one block, in which case the layout (and cost) is identical to the one-frame-per-doc v10.
pub(crate) const TEXT_BLOCK_SIZE: u32 = 16 * 1024;

/// Cut a document's pool (`pool_len` bytes) into block boundaries: cumulative *end* offsets,
/// doc-relative, last always `pool_len`; empty for an empty pool. `ranges` are the pool's
/// text-node ranges in any order (they are always mutually disjoint — the pool only ever
/// appends — so cutting at any range end can never split another range; stale ranges from
/// mutated documents are harmless extra boundaries). Greedy: accumulate ranges until a block
/// reaches [`TEXT_BLOCK_SIZE`], cut at that range's end. The final cut is forced to `pool_len`
/// so tail bytes past the last range (dead pool space) stay covered; a non-empty pool with no
/// ranges at all degenerates to one block.
pub(crate) fn block_cuts(mut ranges: Vec<Range<u32>>, pool_len: u32) -> Vec<u32> {
    if pool_len == 0 {
        return Vec::new();
    }
    ranges.sort_unstable_by_key(|r| r.start);
    let mut cuts = Vec::with_capacity(pool_len.div_ceil(TEXT_BLOCK_SIZE) as usize);
    let mut block_start = 0u32;
    for r in &ranges {
        debug_assert!(r.end <= pool_len, "text range past the pool");
        if r.end - block_start >= TEXT_BLOCK_SIZE {
            cuts.push(r.end);
            block_start = r.end;
        }
    }
    if cuts.last() != Some(&pool_len) {
        cuts.push(pool_len);
    }
    cuts
}

/// The text/comment pools of every document in a bundle, block-split and compressed: every
/// block's zstd frame concatenated into `frames`, with three cumulative offset tables. The two
/// block tables have `block_count + 1` entries — block `b` is the frame
/// `frames[block_offsets[b]..block_offsets[b + 1]]`, inflating to
/// `block_raw_offsets[b + 1] - block_raw_offsets[b]` bytes — and `doc_blocks` has
/// `doc_count + 1` entries mapping document `slot` to its block run
/// `doc_blocks[slot]..doc_blocks[slot + 1]`. Blocks never straddle documents. `block_raw_offsets`
/// is bundle-cumulative (a document's node text ranges — document-local — index its segment,
/// anchored at its first block's raw offset). A text-free document owns zero blocks.
#[derive(Archive, Serialize, Deserialize, Clone)]
pub struct BundleStrings {
    frames: Vec<u8>,
    block_offsets: Vec<u32>,
    block_raw_offsets: Vec<u32>,
    doc_blocks: Vec<u32>,
}

/// An empty store with valid (sentinel-`0`) offset tables, so `push_doc_blocks` and
/// `std::mem::take` are always sound — never bare `Vec::new()`s that would underflow `doc_count`.
impl Default for BundleStrings {
    fn default() -> Self {
        Self::with_doc_capacity(0)
    }
}

impl BundleStrings {
    /// An empty store sized for `docs` documents.
    pub fn with_doc_capacity(docs: usize) -> Self {
        let mut block_offsets = Vec::with_capacity(docs + 1);
        block_offsets.push(0);
        let mut block_raw_offsets = Vec::with_capacity(docs + 1);
        block_raw_offsets.push(0);
        let mut doc_blocks = Vec::with_capacity(docs + 1);
        doc_blocks.push(0);
        Self {
            frames: Vec::new(),
            block_offsets,
            block_raw_offsets,
            doc_blocks,
        }
    }

    /// Append one document's compressed blocks, returning its slot. `frames` is the document's
    /// concatenated block frames; `frame_ends` / `raw_ends` are its doc-relative cumulative block
    /// ends (one entry per block, last `raw_end` = pool length) — the writer/worker produced them
    /// together via [`block_cuts`] + [`crate::codec`]. All empty for a text-free document.
    pub fn push_doc_blocks(
        &mut self,
        frames: &[u8],
        frame_ends: &[u32],
        raw_ends: &[u32],
    ) -> usize {
        debug_assert_eq!(frame_ends.len(), raw_ends.len(), "one frame per raw block");
        debug_assert_eq!(
            frame_ends.last().copied().unwrap_or(0) as usize,
            frames.len(),
            "frame ends must cover the frame bytes"
        );
        let slot = self.doc_blocks.len() - 1;
        let frame_base = *self.block_offsets.last().expect("sentinel");
        let raw_base = *self.block_raw_offsets.last().expect("sentinel");
        self.frames.extend_from_slice(frames);
        for (&fe, &re) in frame_ends.iter().zip(raw_ends) {
            self.block_offsets.push(frame_base + fe);
            self.block_raw_offsets.push(raw_base + re);
        }
        let blocks = *self.doc_blocks.last().expect("sentinel") + frame_ends.len() as u32;
        self.doc_blocks.push(blocks);
        slot
    }

    pub fn doc_count(&self) -> usize {
        self.doc_blocks.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.doc_count() == 0
    }

    /// Total stored (compressed) byte length — the concatenated frames.
    pub fn byte_len(&self) -> usize {
        self.frames.len()
    }
}

/// One document's block tables copied out to native `u32` (fencepost form, `block_count + 1`
/// entries each — typically 2, since most pools fit one block). A read handle owns one of these
/// plus a matching `Box<[OnceLock<Vec<u8>>]>` of inflate caches; together they feed
/// [`LazyState`]. Offsets stay bundle-absolute, exactly as archived.
pub struct DocBlocks {
    pub frame_starts: Box<[u32]>,
    pub raw_starts: Box<[u32]>,
}

impl DocBlocks {
    pub fn block_count(&self) -> usize {
        self.raw_starts.len() - 1
    }

    /// The document's raw segment start within the bundle (== `raw_starts[0]`).
    pub fn base(&self) -> u32 {
        self.raw_starts[0]
    }

    pub fn raw_len(&self) -> u32 {
        self.raw_starts[self.raw_starts.len() - 1] - self.raw_starts[0]
    }

    /// Freshly initialized inflate caches, one per block.
    pub fn bufs(&self) -> Box<[OnceLock<Vec<u8>>]> {
        (0..self.block_count()).map(|_| OnceLock::new()).collect()
    }
}

/// A whole bundle's native-`u32` block tables plus one inflate cache per block, built once per
/// bundle by [`ArchivedBundleStrings::arena`] so a sweep converts the archived (little-endian)
/// tables a single time — not per document — and [`lazy_states`](ArchivedBundleStrings::lazy_states)
/// can hand every document a subslice.
pub struct StringsArena {
    bufs: Box<[OnceLock<Vec<u8>>]>,
    frame_starts: Box<[u32]>,
    raw_starts: Box<[u32]>,
}

impl ArchivedBundleStrings {
    /// The number of documents this block covers.
    pub fn doc_count(&self) -> usize {
        self.doc_blocks.len() - 1
    }

    /// The number of compressed blocks across all documents.
    pub fn block_count(&self) -> usize {
        self.block_offsets.len() - 1
    }

    /// The bundle's concatenated frame blob (all blocks; per-block tables index into it).
    pub fn frames(&self) -> &[u8] {
        &self.frames
    }

    /// Document `slot`'s block run.
    fn doc_block_range(&self, slot: usize) -> Range<usize> {
        self.doc_blocks[slot].to_native() as usize..self.doc_blocks[slot + 1].to_native() as usize
    }

    /// Document `slot`'s decompressed (raw) segment length.
    pub fn raw_len(&self, slot: usize) -> u32 {
        let r = self.doc_block_range(slot);
        self.block_raw_offsets[r.end].to_native() - self.block_raw_offsets[r.start].to_native()
    }

    /// Document `slot`'s block tables, copied to native `u32` for a read handle's lifetime.
    pub fn doc_blocks(&self, slot: usize) -> DocBlocks {
        let r = self.doc_block_range(slot);
        DocBlocks {
            frame_starts: self.block_offsets[r.start..=r.end]
                .iter()
                .map(|o| o.to_native())
                .collect(),
            raw_starts: self.block_raw_offsets[r.start..=r.end]
                .iter()
                .map(|o| o.to_native())
                .collect(),
        }
    }

    /// Decode document `slot`'s entire raw segment (every block, concatenated) — the whole-pool
    /// consumers' path (in-memory rehydration, repackaging, stats probes).
    pub fn materialize_doc(&self, slot: usize, decoder: &dyn FrameDecoder) -> Vec<u8> {
        let r = self.doc_block_range(slot);
        let mut pool = Vec::with_capacity(self.raw_len(slot) as usize);
        for b in r {
            let fs = self.block_offsets[b].to_native() as usize;
            let fe = self.block_offsets[b + 1].to_native() as usize;
            let raw = (self.block_raw_offsets[b + 1].to_native()
                - self.block_raw_offsets[b].to_native()) as usize;
            pool.extend_from_slice(&decoder.decode(&self.frames[fs..fe], raw));
        }
        pool
    }

    /// The bundle's shared read state: native offset tables + one inflate cache per block. Build
    /// once per bundle, then borrow it to [`lazy_states`](Self::lazy_states).
    pub fn arena(&self) -> StringsArena {
        StringsArena {
            bufs: (0..self.block_count()).map(|_| OnceLock::new()).collect(),
            frame_starts: self.block_offsets.iter().map(|o| o.to_native()).collect(),
            raw_starts: self
                .block_raw_offsets
                .iter()
                .map(|o| o.to_native())
                .collect(),
        }
    }

    /// Build a per-document [`LazyState`] for every slot — each a subslice of the shared
    /// `arena` (tables and caches) paired with this block's frame blob and the archive's
    /// `decoder`. The caller owns `arena` and the returned `Vec` as two separate values in its
    /// read scope, so nothing here is self-referential; a bound document inflates a block at
    /// most once, only when text inside it is actually read.
    pub fn lazy_states<'s>(
        &'s self,
        decoder: &'s dyn FrameDecoder,
        arena: &'s StringsArena,
    ) -> Vec<LazyState<'s>> {
        debug_assert_eq!(
            arena.bufs.len(),
            self.block_count(),
            "one inflate cache per block"
        );
        (0..self.doc_count())
            .map(|slot| {
                let r = self.doc_block_range(slot);
                LazyState {
                    bufs: &arena.bufs[r.start..r.end],
                    frames: &self.frames,
                    frame_starts: &arena.frame_starts[r.start..=r.end],
                    raw_starts: &arena.raw_starts[r.start..=r.end],
                    base: arena.raw_starts[r.start],
                    len: arena.raw_starts[r.end] - arena.raw_starts[r.start],
                    decoder,
                }
            })
            .collect()
    }

    /// Validate the offset tables against the frame blob: the two block tables equal-length,
    /// all three cumulative tables non-empty, starting at `0`, monotonic; the frame table ending
    /// exactly at the blob length; and `doc_blocks` ending exactly at the block count. A corrupt
    /// table would otherwise panic (or slice out of range) at read time; surfacing it here keeps
    /// `MmapArchive::open` the single validation gate. (Frame *contents* are checked lazily — a
    /// bad frame panics on inflate, like a bad document blob.)
    pub fn validate(&self) -> bool {
        if self.block_offsets.len() != self.block_raw_offsets.len() {
            return false;
        }
        if self.block_offsets.is_empty()
            || self.doc_blocks.is_empty()
            || self.block_offsets[0].to_native() != 0
            || self.block_raw_offsets[0].to_native() != 0
            || self.doc_blocks[0].to_native() != 0
        {
            return false;
        }
        if !monotonic(self.block_offsets.iter().map(|o| o.to_native()))
            || !monotonic(self.block_raw_offsets.iter().map(|o| o.to_native()))
            || !monotonic(self.doc_blocks.iter().map(|o| o.to_native()))
        {
            return false;
        }
        self.block_offsets[self.block_offsets.len() - 1].to_native() as usize == self.frames.len()
            && self.doc_blocks[self.doc_blocks.len() - 1].to_native() as usize
                == self.block_offsets.len() - 1
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
    use crate::codec::ZstdFrameDecoder;
    use crate::codec::{build_compressor, compress_segment};
    use rkyv::rancor::Error;

    /// Compress one doc's pool per `block_cuts`, returning what the writer hands to
    /// `push_doc_blocks`.
    fn compress_doc(raw: &[u8], ranges: Vec<Range<u32>>) -> (Vec<u8>, Vec<u32>, Vec<u32>) {
        let raw_ends = block_cuts(ranges, raw.len() as u32);
        let mut c = build_compressor().unwrap();
        let mut frames = Vec::new();
        let mut frame_ends = Vec::new();
        let mut start = 0u32;
        for &end in &raw_ends {
            let frame = compress_segment(&mut c, &raw[start as usize..end as usize]).unwrap();
            frames.extend_from_slice(&frame);
            frame_ends.push(frames.len() as u32);
            start = end;
        }
        (frames, frame_ends, raw_ends)
    }

    /// Round-trip three documents (including an empty middle one and a multi-block last one)
    /// through block cutting + compression + the archived offset tables + the decoder,
    /// recovering each segment exactly.
    #[test]
    fn round_trip_segments_across_docs() {
        // Doc 2 is two nodes straddling the block target: cut at the first node's end.
        let big_a = "a".repeat(TEXT_BLOCK_SIZE as usize + 100);
        let big = format!("{big_a}tail tail tail");
        let docs: [(&[u8], Vec<Range<u32>>); 3] = [
            (b"alpha alpha alpha", vec![0..10, 10..17]),
            (b"", vec![]),
            (
                big.as_bytes(),
                vec![0..big_a.len() as u32, big_a.len() as u32..big.len() as u32],
            ),
        ];

        let mut bs = BundleStrings::with_doc_capacity(3);
        for (slot, (raw, ranges)) in docs.iter().enumerate() {
            let (frames, frame_ends, raw_ends) = compress_doc(raw, ranges.clone());
            assert_eq!(bs.push_doc_blocks(&frames, &frame_ends, &raw_ends), slot);
        }
        assert_eq!(bs.doc_count(), 3);

        let bytes = rkyv::to_bytes::<Error>(&bs).unwrap();
        let arch = rkyv::access::<ArchivedBundleStrings, Error>(&bytes).unwrap();
        assert!(arch.validate());
        assert_eq!(arch.doc_count(), 3);
        assert_eq!(arch.block_count(), 3, "1 + 0 + 2 blocks");

        let decoder = ZstdFrameDecoder::new(None);
        for (slot, (raw, _)) in docs.iter().enumerate() {
            assert_eq!(arch.raw_len(slot) as usize, raw.len());
            assert_eq!(arch.materialize_doc(slot, &decoder), *raw);
        }
        // The empty middle document owns zero blocks.
        assert_eq!(arch.doc_blocks(1).block_count(), 0);

        // Selective read through the shared arena: only the touched block inflates.
        let arena = arch.arena();
        let states = arch.lazy_states(&decoder, &arena);
        assert_eq!(states[2].bufs.len(), 2);
        assert_eq!(states[2].base, 17, "doc 2's segment sits after doc 0's");
    }

    #[test]
    fn block_cuts_edges() {
        // Empty pool: no blocks.
        assert!(block_cuts(vec![], 0).is_empty());
        // Non-empty pool with no live ranges (all text nodes removed): one block.
        assert_eq!(block_cuts(vec![], 100), vec![100]);
        // Small pool: one block regardless of range count.
        assert_eq!(block_cuts(vec![0..3, 3..9], 9), vec![9]);
        // Out-of-order (mutated doc) ranges are sorted before cutting; tail garbage past the
        // last range is folded into the final block.
        let n = TEXT_BLOCK_SIZE;
        assert_eq!(
            block_cuts(vec![(n + 5)..(n + 9), 0..n, n..(n + 5)], n + 20),
            vec![n, n + 20]
        );
        // A single oversized node is one oversized block.
        assert_eq!(block_cuts(vec![0..3, 3..(3 * n)], 3 * n), vec![3 * n]);
    }

    /// A corrupt `doc_blocks` table (claiming more blocks than exist) must fail validation, not
    /// panic at read time.
    #[test]
    fn corrupt_doc_blocks_rejected() {
        let mut bs = BundleStrings::with_doc_capacity(1);
        let (frames, frame_ends, raw_ends) = compress_doc(b"hello world", vec![0..5, 5..11]);
        bs.push_doc_blocks(&frames, &frame_ends, &raw_ends);
        bs.doc_blocks.push(7); // one phantom document pointing past the block table
        let bytes = rkyv::to_bytes::<Error>(&bs).unwrap();
        let arch = rkyv::access::<ArchivedBundleStrings, Error>(&bytes).unwrap();
        assert!(!arch.validate());
    }
}
