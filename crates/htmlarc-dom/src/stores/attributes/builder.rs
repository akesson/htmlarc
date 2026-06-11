use std::hash::BuildHasher;

use hashbrown::{DefaultHashBuilder, HashTable};

use crate::html::HtmlAttr;
use crate::stores::{ListIndex, listvec::ListVec, stringheap::StringHeap};

use super::AttributeStore;

/// Builds the attribute store during parsing.
///
/// Each distinct `(tag, value)` is copied **once**, straight into the [`StringHeap`] —
/// there is no `Box<str>` intermediary. A [`HashTable`] of `u16` heap indices
/// deduplicates `(tag, value)` pairs, confirming equality against `tags` + the heap, so a
/// repeated attribute allocates nothing. The values arrive already entity-decoded (the
/// tokenizer resolves character references), so they are interned verbatim.
///
/// The runtime [`AttributeStore`] binary-searches its `(tag, value)` table, so [`build`]
/// sorts the index table into `(tag, value)` order. The heap bytes themselves stay in
/// first-seen order — the table indexes into them, so no value is copied a second time.
///
/// [`build`]: Self::build
#[derive(Default)]
pub struct AttributeStoreBuilder {
    lists: ListVec,
    /// One entry per distinct `(tag, value)`, in first-seen order.
    heap: StringHeap,
    /// `tags[i]` is the attribute tag of heap entry `i` (parallel to `heap`).
    tags: Vec<u8>,
    /// Dedup table: `(tag, value)` hash -> heap index.
    table: HashTable<u16>,
    hasher: DefaultHashBuilder,
    /// Set (first reason wins) once a per-document u16 ceiling is hit; the parse path
    /// reads it via [`overflow`](Self::overflow) and discards the whole document.
    overflow: Option<&'static str>,
}

impl AttributeStoreBuilder {
    /// The reason this builder overflowed a per-document capacity, if any.
    pub fn overflow(&self) -> Option<&'static str> {
        self.overflow
    }

    pub fn new_list(&mut self, tag: HtmlAttr, val: &str) -> ListIndex {
        let i = self.get_or_insert(tag, val);
        match self.lists.try_new_list(i) {
            Some(list) => list,
            None => {
                self.overflow
                    .get_or_insert("attribute list count exceeds 65,534");
                ListIndex::from(0)
            }
        }
    }

    pub fn add_attribute(&mut self, list_index: ListIndex, tag: HtmlAttr, val: &str) {
        let i = self.get_or_insert(tag, val);
        if !self.lists.list_mut_at(list_index).try_append(i) {
            self.overflow
                .get_or_insert("attribute list entries exceed 32,768");
        }
    }

    fn get_or_insert(&mut self, tag: HtmlAttr, val: &str) -> u16 {
        let Self {
            heap,
            tags,
            table,
            hasher,
            overflow,
            ..
        } = self;
        let tag = tag as u8;
        let hash = hash_attr(hasher, tag, val);
        if let Some(&i) = table.find(hash, |&i| tags[i as usize] == tag && &heap[i] == val) {
            return i;
        }
        let Some(i) = heap.try_insert(val) else {
            overflow.get_or_insert("attribute value strings exceed 65,535");
            return 0;
        };
        tags.push(tag);
        table.insert_unique(hash, i, |&j| hash_attr(hasher, tags[j as usize], &heap[j]));
        i
    }

    pub fn build(self) -> AttributeStore {
        let AttributeStoreBuilder {
            mut lists,
            heap,
            tags,
            ..
        } = self;

        // Sort the heap indices into `(tag, value)` order — what the runtime store's binary
        // search expects. The heap bytes stay in insertion order; the table just points back
        // into them by their original index, so each value is copied only once (at parse).
        let mut order: Vec<u16> = (0..heap.len()).collect();
        order.sort_unstable_by(|&a, &b| {
            (tags[a as usize], &heap[a]).cmp(&(tags[b as usize], &heap[b]))
        });

        let mut reidx = vec![0u16; order.len()];
        let mut attributes: Vec<(u8, u16)> = Vec::with_capacity(order.len());
        for (new_index, &old) in order.iter().enumerate() {
            reidx[old as usize] = new_index as u16;
            attributes.push((tags[old as usize], old));
        }

        lists.reindex_value(&reidx);
        AttributeStore {
            lists,
            attributes,
            strings: heap,
        }
    }
}

fn hash_attr(hasher: &DefaultHashBuilder, tag: u8, val: &str) -> u64 {
    hasher.hash_one((tag, val))
}
