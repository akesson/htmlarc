use std::hash::BuildHasher;

use hashbrown::{DefaultHashBuilder, HashTable};

use super::stringheap::StringHeap;

/// Interns `&str` values into a [`StringHeap`], copying each distinct value exactly
/// once. The dedup table stores only `u16` heap indices and confirms equality by
/// comparing against the heap, so there is no per-value `Box<str>` allocation.
///
/// Values land in the heap in first-seen order; the owning store sorts its *index table*
/// at build time (which is all its binary search needs), leaving the heap bytes in
/// insertion order. That layout is fully deterministic for a given input. A future archive
/// compressor could reorder the heap to cluster similar strings (worth ~1–2.5% on the
/// compressed archive in measurements), but that belongs at serialize time, not here.
#[derive(Default)]
pub(crate) struct StringInterner {
    heap: StringHeap,
    table: HashTable<u16>,
    hasher: DefaultHashBuilder,
}

impl StringInterner {
    /// Returns the index of `s`, interning it (one copy into the heap) on first sight.
    pub(crate) fn intern(&mut self, s: &str) -> u16 {
        let Self {
            heap,
            table,
            hasher,
        } = self;
        let hash = hasher.hash_one(s);
        if let Some(&i) = table.find(hash, |&i| &heap[i] == s) {
            return i;
        }
        let i = heap.insert(s);
        table.insert_unique(hash, i, |&j| hasher.hash_one(&heap[j]));
        i
    }

    pub(crate) fn get(&self, index: u16) -> &str {
        &self.heap[index]
    }

    pub(crate) fn len(&self) -> u16 {
        self.heap.len()
    }

    /// Consumes the interner, yielding the heap (insertion order).
    pub(crate) fn into_heap(self) -> StringHeap {
        self.heap
    }
}
