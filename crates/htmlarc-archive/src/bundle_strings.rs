//! Per-bundle document-string store.
//!
//! Each document's text/comment pool used to live inline in its own rkyv blob. Relocating every
//! pool in a bundle into one [`BundleStrings`] block (written into the bundle's reserved data
//! region, see [`crate::writer`]) is what lets a whole bundle share one storage decision: today
//! the block is stored *uncompressed*, so a reader borrows each document's segment zero-copy
//! ([`StringSource::Plain`]); a later step can compress the block and hand out a lazily-inflated
//! source without the document knowing.

use htmlarc_dom::prelude::StringSource;
use rkyv::{Archive, Deserialize, Serialize};

/// The text/comment pools of every document in a bundle, concatenated into one byte block with a
/// per-document offset table. `doc_offsets` has `doc_count + 1` entries (cumulative byte lengths);
/// document `slot` owns `bytes[doc_offsets[slot]..doc_offsets[slot + 1]]`, and its node text
/// ranges — which stay document-local — index directly into that segment.
#[derive(Archive, Serialize, Deserialize, Clone)]
pub struct BundleStrings {
    bytes: Vec<u8>,
    doc_offsets: Vec<u32>,
}

/// An empty store with a valid (sentinel-`0`) offset table, so `push_doc` and `std::mem::take`
/// are always sound — never a bare `Vec::new()` that would underflow `doc_count`.
impl Default for BundleStrings {
    fn default() -> Self {
        Self::with_doc_capacity(0)
    }
}

impl BundleStrings {
    /// An empty store sized for `docs` documents.
    pub fn with_doc_capacity(docs: usize) -> Self {
        let mut doc_offsets = Vec::with_capacity(docs + 1);
        doc_offsets.push(0);
        Self {
            bytes: Vec::new(),
            doc_offsets,
        }
    }

    /// Append one document's text segment (its former inline pool), returning its slot.
    pub fn push_doc(&mut self, segment: &[u8]) -> usize {
        let slot = self.doc_offsets.len() - 1;
        self.bytes.extend_from_slice(segment);
        self.doc_offsets.push(self.bytes.len() as u32);
        slot
    }

    pub fn doc_count(&self) -> usize {
        self.doc_offsets.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.doc_count() == 0
    }

    /// Total stored byte length (the concatenated pools).
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

impl ArchivedBundleStrings {
    /// The number of documents this block covers.
    pub fn doc_count(&self) -> usize {
        self.doc_offsets.len() - 1
    }

    /// Document `slot`'s raw text segment.
    pub fn segment(&self, slot: usize) -> &[u8] {
        let start = self.doc_offsets[slot].to_native() as usize;
        let end = self.doc_offsets[slot + 1].to_native() as usize;
        &self.bytes[start..end]
    }

    /// A [`StringSource`] over document `slot`'s segment — zero-copy (`Plain`) while the block is
    /// stored uncompressed. Panics on a `slot` past the table (callers index within a validated
    /// bundle range).
    pub fn source_for(&self, slot: usize) -> StringSource<'_> {
        StringSource::plain(self.segment(slot))
    }

    /// Validate the offset table against the byte block: monotonic, in-bounds, and ending exactly
    /// at the block length. A corrupt table would otherwise panic (or slice out of range) at read
    /// time; surfacing it here keeps `MmapArchive::open` the single validation gate.
    pub fn validate(&self) -> bool {
        if self.doc_offsets.is_empty() || self.doc_offsets[0].to_native() != 0 {
            return false;
        }
        let mut prev = 0u32;
        for off in self.doc_offsets.iter() {
            let v = off.to_native();
            if v < prev {
                return false;
            }
            prev = v;
        }
        prev as usize == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rkyv::rancor::Error;

    #[test]
    fn round_trip_segments_across_docs() {
        let mut bs = BundleStrings::with_doc_capacity(3);
        assert_eq!(bs.push_doc(b"alpha"), 0);
        assert_eq!(bs.push_doc(b""), 1); // an empty (text-free) document
        assert_eq!(bs.push_doc(b"gamma!"), 2);
        assert_eq!(bs.doc_count(), 3);

        let bytes = rkyv::to_bytes::<Error>(&bs).unwrap();
        let arch = rkyv::access::<ArchivedBundleStrings, Error>(&bytes).unwrap();
        assert!(arch.validate());
        assert_eq!(arch.doc_count(), 3);

        // Each document's segment is recovered exactly, including the empty middle one.
        assert_eq!(arch.segment(0), b"alpha");
        assert_eq!(arch.segment(1), b"");
        assert_eq!(arch.segment(2), b"gamma!");
    }
}
