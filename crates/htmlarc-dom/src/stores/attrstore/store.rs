use rkyv::{Archive, Deserialize, Serialize};

use crate::html::HtmlAttr;
use crate::stores::symbols::{SHARED_BASE, Sym, SymbolTable, SymbolTableView};
use crate::stores::{RunIndex, RunValues, RunVec, RunVecView};

use super::{AttrName, Attribute, NAME_EXT_BASE};

// The extended-name bias never reaches the reserved shared range or the sentinel: the
// largest local sym is `LOCAL_CAP - 1`, so the largest `NameSym` is `LOCAL_CAP - 1 + 256`.
const _: () = assert!(
    (crate::stores::symbols::LOCAL_CAP - 1) as u32 + NAME_EXT_BASE as u32 == SHARED_BASE as u32 - 1
);

/// Decode a stored `NameSym` into a borrowed [`AttrName`]: below [`NAME_EXT_BASE`] it is a
/// standard [`HtmlAttr`]; above, `(sym - NAME_EXT_BASE)` indexes the document symbol table.
pub(crate) fn decode_name(name_sym: u16, symbols: SymbolTableView<'_>) -> AttrName<'_> {
    if name_sym < NAME_EXT_BASE {
        AttrName::Std(
            HtmlAttr::from_repr(name_sym as u8).expect("std name sym is a valid HtmlAttr"),
        )
    } else {
        AttrName::Ext(symbols.get(Sym(name_sym - NAME_EXT_BASE)))
    }
}

/// One per-document attribute store (ADR 0002 §3).
///
/// Each distinct attribute is an entry: a `(NameSym, ValueRef)` pair with a stable,
/// insertion-ordered id. `values` holds the value strings (dedup + content permutation);
/// `entries` are the pairs; `sorted` is their numeric `(NameSym, ValueRef)` permutation, so
/// [`find_entry`](AttrStoreView::find_entry) is a binary search with no string deref; `lists`
/// holds each element's entry ids as a contiguous run.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct AttrStore {
    pub(crate) values: SymbolTable,
    pub(crate) entries: Vec<(u16, u16)>,
    /// Numeric `(NameSym, ValueRef)` permutation of `0..entries.len()`.
    pub(crate) sorted: Vec<u16>,
    pub(crate) lists: RunVec,
}

impl AttrStore {
    pub(crate) fn from_parts(
        values: SymbolTable,
        entries: Vec<(u16, u16)>,
        sorted: Vec<u16>,
        lists: RunVec,
    ) -> Self {
        Self {
            values,
            entries,
            sorted,
            lists,
        }
    }

    /// The `(NameSym, ValueRef)` pair of an entry id.
    pub(crate) fn entry(&self, id: u16) -> (u16, u16) {
        self.entries[id as usize]
    }

    /// Live-mutation get-or-create of the entry for `(name_sym, value)`: interns the value,
    /// then binary-searches the numeric permutation for the pair, appending a new stable
    /// entry (and one permutation memmove) on a miss. Panics at the per-document ceiling —
    /// mutable documents go wide in ADR 0002 PR 6; until then it is a hard error, matching
    /// the symbol table and class arena.
    pub(crate) fn entry_or_insert(&mut self, name_sym: u16, value: &str) -> u16 {
        let vref = self.values.get_or_insert(value).as_u16();
        let pair = (name_sym, vref);
        match self.view().search(pair) {
            Ok(id) => id,
            Err(pos) => {
                let id = self.entries.len() as u16;
                self.entries.push(pair);
                self.sorted.insert(pos, id);
                id
            }
        }
    }

    /// Live mutation: start a new attribute run for this element holding `(name_sym, value)`.
    pub(crate) fn new_run(&mut self, name_sym: u16, value: &str) -> RunIndex {
        let id = self.entry_or_insert(name_sym, value);
        self.lists.new_run(id)
    }

    /// Live mutation: append `(name_sym, value)` to the run at `start`, returning its
    /// (possibly relocated) start — the caller re-points the node slot when it differs.
    pub(crate) fn append(&mut self, start: RunIndex, name_sym: u16, value: &str) -> RunIndex {
        let id = self.entry_or_insert(name_sym, value);
        self.lists.append(start, id)
    }

    pub(crate) fn view(&self) -> AttrStoreView<'_> {
        AttrStoreView {
            values: self.values.view(),
            entries: EntriesView::Owned(&self.entries),
            sorted: SortedView::Owned(&self.sorted),
            lists: self.lists.view(),
        }
    }
}

/// The `(NameSym, ValueRef)` entry table: owned native pairs, or archived little-endian
/// ones read via `.to_native()`.
#[derive(Clone, Copy)]
enum EntriesView<'a> {
    Owned(&'a [(u16, u16)]),
    Archived(&'a [rkyv::Archived<(u16, u16)>]),
}

impl EntriesView<'_> {
    fn at(&self, index: usize) -> (u16, u16) {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => (s[index].0.to_native(), s[index].1.to_native()),
        }
    }
    #[cfg(test)]
    fn len(&self) -> usize {
        match self {
            Self::Owned(s) => s.len(),
            Self::Archived(s) => s.len(),
        }
    }
}

/// The numeric permutation: owned native `u16`s or archived little-endian ones.
#[derive(Clone, Copy)]
enum SortedView<'a> {
    Owned(&'a [u16]),
    Archived(&'a [rkyv::Archived<u16>]),
}

impl SortedView<'_> {
    fn at(&self, index: usize) -> u16 {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => s[index].to_native(),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Owned(s) => s.len(),
            Self::Archived(s) => s.len(),
        }
    }
}

/// Borrowed, read-only view over an [`AttrStore`] — owned or rkyv-archived.
#[derive(Clone, Copy)]
pub(crate) struct AttrStoreView<'a> {
    values: SymbolTableView<'a>,
    entries: EntriesView<'a>,
    sorted: SortedView<'a>,
    lists: RunVecView<'a>,
}

impl<'a> AttrStoreView<'a> {
    /// Deref an entry id into a borrowed [`Attribute`] (name resolved via `symbols`).
    pub(crate) fn attribute_at(&self, id: u16, symbols: SymbolTableView<'a>) -> Attribute<'a> {
        let (name_sym, vref) = self.entries.at(id as usize);
        Attribute {
            name: decode_name(name_sym, symbols),
            val: self.values.get(Sym(vref)),
        }
    }

    /// Iterate the entry ids of the run at `start`.
    pub(crate) fn run_at(&self, start: RunIndex) -> RunValues<'a> {
        self.lists.run_at(start)
    }

    /// Advance an externally-held run cursor (an arena offset), yielding the next entry id.
    pub(crate) fn next_entry_in_run(&self, offset: &mut Option<u16>) -> Option<u16> {
        self.lists.next_in_run(offset)
    }

    /// Binary-search the numeric permutation for `(NameSym, ValueRef)`. `Ok(id)` is the
    /// matching stable entry id; `Err(pos)` is the permutation slot a new entry would take.
    pub(crate) fn search(&self, pair: (u16, u16)) -> Result<u16, usize> {
        use std::cmp::Ordering::*;
        let mut left = 0usize;
        let mut right = self.sorted.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let id = self.sorted.at(mid);
            match self.entries.at(id as usize).cmp(&pair) {
                Less => left = mid + 1,
                Greater => right = mid,
                Equal => return Ok(id),
            }
        }
        Err(left)
    }
}

// The resolve-once attribute and id matching (ADR 0002 §3): `name_sym` is the per-node
// integer name prefilter; `value_ref`/`find_entry` resolve a selector's value/pair to refs
// once per document.
impl<'a> AttrStoreView<'a> {
    /// The `NameSym` of an entry.
    pub(crate) fn name_sym(&self, id: u16) -> u16 {
        self.entries.at(id as usize).0
    }

    /// Resolve a value string to its `ValueRef`, or `None` if the document never stored it.
    pub(crate) fn value_ref(&self, value: &str) -> Option<u16> {
        self.values.find(value).map(Sym::as_u16)
    }

    /// Resolve a `(NameSym, ValueRef)` pair to its stable entry id, or `None` if absent.
    pub(crate) fn find_entry(&self, pair: (u16, u16)) -> Option<u16> {
        self.search(pair).ok()
    }

    /// The value string of an entry — only the unit tests need this; the query layer derefs
    /// through [`attribute_at`](Self::attribute_at).
    #[cfg(test)]
    pub(crate) fn value_at(&self, id: u16) -> &'a str {
        self.values.get(Sym(self.entries.at(id as usize).1))
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

impl ArchivedAttrStore {
    pub(crate) fn view(&self) -> AttrStoreView<'_> {
        AttrStoreView {
            values: self.values.view(),
            entries: EntriesView::Archived(self.entries.as_slice()),
            sorted: SortedView::Archived(self.sorted.as_slice()),
            lists: self.lists.view(),
        }
    }
}
