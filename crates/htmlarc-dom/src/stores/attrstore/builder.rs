use std::hash::BuildHasher;

use hashbrown::{DefaultHashBuilder, HashTable};

use crate::stores::interner::StringInterner;
use crate::stores::symbols::{LOCAL_CAP, SymbolTable};
use crate::stores::{RunIndex, RunVec};

use super::AttrStore;

/// Builds the attribute store during parsing (ADR 0002 §3).
///
/// Values are interned once into a [`StringInterner`]; a [`HashTable`] of entry ids
/// deduplicates the `(NameSym, ValueRef)` pairs. Each element's attributes are written as a
/// contiguous run of **stable** entry ids into the [`RunVec`] arena — so unlike the former
/// `AttributeStoreBuilder`, [`build`](Self::build) does **not** re-point the runs: it only
/// computes the numeric permutation `find_entry` searches (the value-reindex pass is gone).
#[derive(Default)]
pub(crate) struct AttrStoreBuilder {
    /// Value strings, deduplicated and copied once each.
    values: StringInterner,
    /// Distinct `(NameSym, ValueRef)` pairs, in first-seen (stable id) order.
    entries: Vec<(u16, u16)>,
    /// Dedup table: `(NameSym, ValueRef)` hash -> entry id.
    table: HashTable<u16>,
    hasher: DefaultHashBuilder,
    lists: RunVec,
    /// Set (first reason wins) once a per-document ceiling is hit; the parse path reads it
    /// via [`overflow`](Self::overflow) and discards the whole document.
    overflow: Option<&'static str>,
}

impl AttrStoreBuilder {
    /// The reason this builder overflowed a per-document capacity, if any.
    pub(crate) fn overflow(&self) -> Option<&'static str> {
        self.overflow
    }

    /// Get-or-create the entry id for `(name_sym, value)`, poisoning on a value- or
    /// entry-table overflow (returns a bogus `0` the discarded document never observes).
    fn entry(&mut self, name_sym: u16, value: &str) -> u16 {
        let Some(vref) = self.values.try_intern_capped(value, LOCAL_CAP) else {
            self.overflow
                .get_or_insert("attribute value strings exceed 61,184");
            return 0;
        };
        let Self {
            entries,
            table,
            hasher,
            overflow,
            ..
        } = self;
        let pair = (name_sym, vref);
        let hash = hasher.hash_one(pair);
        if let Some(&id) = table.find(hash, |&i| entries[i as usize] == pair) {
            return id;
        }
        if entries.len() >= LOCAL_CAP as usize {
            overflow.get_or_insert("attribute entries exceed 61,184");
            return 0;
        }
        let id = entries.len() as u16;
        entries.push(pair);
        table.insert_unique(hash, id, |&j| hasher.hash_one(entries[j as usize]));
        id
    }

    /// Start a new attribute run for an element with its first attribute.
    pub(crate) fn new_run(&mut self, name_sym: u16, value: &str) -> RunIndex {
        let id = self.entry(name_sym, value);
        match self.lists.try_new_run(id) {
            Some(run) => run,
            None => {
                self.overflow
                    .get_or_insert("attribute lists exceed 65,535 arena entries");
                RunIndex::from(0)
            }
        }
    }

    /// Append an attribute to the trailing (current element's) run.
    pub(crate) fn append_last(&mut self, start: RunIndex, name_sym: u16, value: &str) {
        let id = self.entry(name_sym, value);
        if !self.lists.try_append_last(start, id) {
            self.overflow
                .get_or_insert("attribute lists exceed 65,535 arena entries");
        }
    }

    pub(crate) fn build(self) -> AttrStore {
        let AttrStoreBuilder {
            values,
            entries,
            lists,
            ..
        } = self;
        // Runs already hold stable entry ids, so only the numeric permutation is computed
        // here — there is no run reindex (cf. the deleted `ListVec::reindex_value`).
        let mut sorted: Vec<u16> = (0..entries.len() as u16).collect();
        sorted.sort_unstable_by_key(|&id| entries[id as usize]);
        AttrStore::from_parts(SymbolTable::from_interner(values), entries, sorted, lists)
    }
}
