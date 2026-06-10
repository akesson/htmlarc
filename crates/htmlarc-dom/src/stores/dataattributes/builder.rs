use std::hash::BuildHasher;

use hashbrown::{DefaultHashBuilder, HashTable};

use crate::stores::interner::StringInterner;
use crate::stores::{ListIndex, listvec::ListVec};

use super::{DataAttribute, DataAttributeStore};

/// Builds the data-attribute store during parsing.
///
/// Unlike the previous design — which inserted straight into the live, sorted
/// [`DataAttributeStore`] on every attribute, paying an `O(n)` `Vec::insert` plus a full
/// list-vector walk per new value (so `O(n²)` on data-attribute-heavy documents) — this
/// accumulates during parsing and sorts once at [`build`](Self::build).
///
/// Both the tag names and the values are interned into a single [`StringInterner`], so a
/// `data-mw` tag repeated across hundreds of distinct values is stored **once** (the old
/// store re-inserted the tag string for every pair). A separate [`HashTable`]
/// deduplicates the `(tag, value)` pairs themselves.
#[derive(Default)]
pub struct DataAttributeStoreBuilder {
    lists: ListVec,
    /// Tag names and values, deduplicated and copied once each.
    strings: StringInterner,
    /// Distinct `(tag_index, value_index)` pairs, in first-seen order.
    pairs: Vec<(u16, u16)>,
    /// Dedup table: `(tag_index, value_index)` hash -> pair index.
    table: HashTable<u16>,
    hasher: DefaultHashBuilder,
    /// Set (first reason wins) once a per-document u16 ceiling is hit; the parse path
    /// reads it via [`overflow`](Self::overflow) and discards the whole document.
    overflow: Option<&'static str>,
}

impl DataAttributeStoreBuilder {
    /// The reason this builder overflowed a per-document capacity, if any.
    pub fn overflow(&self) -> Option<&'static str> {
        self.overflow
    }

    pub fn add_list(&mut self, attr: &DataAttribute) -> ListIndex {
        let i = self.get_or_insert(attr);
        match self.lists.try_new_list(i) {
            Some(list) => list,
            None => {
                self.overflow
                    .get_or_insert("data-attribute list count exceeds 65,534");
                ListIndex::from(0)
            }
        }
    }

    pub fn add_attribute(&mut self, list_index: ListIndex, attr: &DataAttribute) {
        let i = self.get_or_insert(attr);
        if !self.lists.list_mut_at(list_index).try_append(i) {
            self.overflow
                .get_or_insert("data-attribute list entries exceed 32,768");
        }
    }

    fn get_or_insert(&mut self, attr: &DataAttribute) -> u16 {
        let (Some(tag), Some(val)) = (
            self.strings.try_intern(attr.tag),
            self.strings.try_intern(attr.val),
        ) else {
            self.overflow
                .get_or_insert("data-attribute strings exceed 65,535");
            return 0;
        };

        let Self {
            pairs,
            table,
            hasher,
            overflow,
            ..
        } = self;
        let hash = hasher.hash_one((tag, val));
        if let Some(&i) = table.find(hash, |&i| pairs[i as usize] == (tag, val)) {
            return i;
        }
        if pairs.len() >= u16::MAX as usize {
            overflow.get_or_insert("data-attribute pairs exceed 65,535");
            return 0;
        }
        let i = pairs.len() as u16;
        pairs.push((tag, val));
        table.insert_unique(hash, i, |&j| hasher.hash_one(pairs[j as usize]));
        i
    }

    pub fn build(self) -> DataAttributeStore {
        let DataAttributeStoreBuilder {
            mut lists,
            strings,
            pairs,
            ..
        } = self;

        // Sort the pairs into `DataAttribute` order — `(tag, value)` by string content —
        // which is what the runtime store's binary search expects.
        let mut order: Vec<u16> = (0..pairs.len() as u16).collect();
        order.sort_unstable_by(|&a, &b| {
            let (ta, va) = pairs[a as usize];
            let (tb, vb) = pairs[b as usize];
            (strings.get(ta), strings.get(va)).cmp(&(strings.get(tb), strings.get(vb)))
        });

        let mut reidx = vec![0u16; pairs.len()];
        let mut attributes: Vec<(u16, u16)> = Vec::with_capacity(pairs.len());
        for (new_index, &old) in order.iter().enumerate() {
            reidx[old as usize] = new_index as u16;
            attributes.push(pairs[old as usize]);
        }

        lists.reindex_value(&reidx);
        DataAttributeStore {
            lists,
            attributes,
            strings: strings.into_heap(),
        }
    }
}
