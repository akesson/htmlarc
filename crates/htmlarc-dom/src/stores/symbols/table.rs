use rkyv::{Archive, Deserialize, Serialize};

use crate::stores::stringheap::{StringHeap, StringHeapView};

use super::{LOCAL_CAP, Sym};

/// One per-document table of deduplicated identity strings (ADR 0002 §1).
///
/// A [`Sym`] is an index into `heap`, assigned in insertion order and stable for the
/// table's lifetime. `sorted` is a content-ordered permutation of those ids, so
/// [`find`](Self::find) is a binary search and a live insert keeps it sorted with a single
/// `Vec::insert` memmove — there is no value reindexing (cf. the deleted
/// `ListVec::shift_values_from`, which existed only because class lists used to store
/// sort positions rather than stable ids).
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub(crate) struct SymbolTable {
    heap: StringHeap,
    /// Content-sorted permutation of `0..heap.len()`.
    sorted: Vec<u16>,
}

impl SymbolTable {
    pub(super) fn from_parts(heap: StringHeap, sorted: Vec<u16>) -> Self {
        Self { heap, sorted }
    }

    pub(crate) fn len(&self) -> u16 {
        self.heap.len()
    }

    pub(crate) fn get(&self, sym: Sym) -> &str {
        &self.heap[sym.as_u16()]
    }

    /// Resolve a string to its stable [`Sym`]. Only the unit tests need the owned form;
    /// the query layer resolves through a [`SymbolTableView`] (owned or archived).
    #[cfg(test)]
    pub(crate) fn find(&self, s: &str) -> Option<Sym> {
        self.view().find(s)
    }

    /// Live-mutation insert: returns the existing [`Sym`] if `s` is present, else appends
    /// it to the heap (taking the next stable id) and memmoves one slot into the sorted
    /// permutation. Returns `None` only when the document already holds `LOCAL_CAP`
    /// distinct symbols (an existing string still resolves at the cap).
    pub(crate) fn try_get_or_insert(&mut self, s: &str) -> Option<Sym> {
        match self.view().search(s) {
            Ok(sym) => Some(sym),
            Err(pos) => {
                if self.heap.len() >= LOCAL_CAP {
                    return None;
                }
                // Bounded by LOCAL_CAP < the heap's own 0xFFFF ceiling, so this can't wrap.
                let sym = self.heap.insert(s);
                self.sorted.insert(pos, sym);
                Some(Sym(sym))
            }
        }
    }

    /// Panicking [`try_get_or_insert`](Self::try_get_or_insert) for the live-mutation API.
    /// Mutable documents go wide in PR 6; until then a per-document ceiling is a hard
    /// error, matching [`StringHeap::insert`].
    pub(crate) fn get_or_insert(&mut self, s: &str) -> Sym {
        self.try_get_or_insert(s)
            .expect("SymbolTable overflow: more than 61,184 distinct symbols in one document")
    }

    /// Rebuild-path compaction (called from the document repackage). `value_reidx` is the
    /// [`crate::stores::listvec::ListRebuilder`] output: indexed by old [`Sym`], it gives
    /// the dense new id of each *used* symbol (numbered in ascending old-id order) or
    /// `None` for the dropped ones. Re-inserting the used strings in that same ascending
    /// order reproduces exactly those new ids, and filtering the old (content-sorted)
    /// permutation through `value_reidx` keeps it sorted without re-sorting.
    pub(crate) fn rebuilt(&self, value_reidx: &[Option<u16>]) -> Self {
        let view = self.view();
        let mut heap = StringHeap::with_capacity_as(&self.heap);
        for (old, slot) in value_reidx.iter().enumerate() {
            if slot.is_some() {
                heap.insert(view.get(Sym(old as u16)));
            }
        }
        let sorted: Vec<u16> = self
            .sorted
            .iter()
            .filter_map(|&old| value_reidx[old as usize])
            .collect();
        let rebuilt = Self::from_parts(heap, sorted);
        debug_assert!(
            rebuilt.is_content_sorted(),
            "rebuilt permutation must stay content-sorted"
        );
        rebuilt
    }

    // Always compiled (not `#[cfg(debug_assertions)]`): `debug_assert!` expands its
    // condition with a runtime `cfg!`, so the call site exists in release builds too — it
    // is just never taken. The body is optimized out when debug assertions are off.
    fn is_content_sorted(&self) -> bool {
        let view = self.view();
        self.sorted
            .windows(2)
            .all(|w| view.get(Sym(w[0])) <= view.get(Sym(w[1])))
    }

    /// The symbol strings in permutation (content-sorted) order — lets a test assert the
    /// sort invariant directly rather than only through `find`'s behaviour.
    #[cfg(test)]
    pub(super) fn permutation_strings(&self) -> Vec<&str> {
        let view = self.view();
        self.sorted.iter().map(|&sym| view.get(Sym(sym))).collect()
    }

    pub(crate) fn view(&self) -> SymbolTableView<'_> {
        SymbolTableView {
            strings: self.heap.view(),
            sorted: SortedView::Owned(&self.sorted),
        }
    }
}

/// The content-sorted permutation: owned native `u16`s, or archived little-endian ones
/// read via `.to_native()`. Erasing the distinction lets one view serve both.
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

/// Borrowed, read-only view over a [`SymbolTable`] — works over both the owned and the
/// rkyv-archived representation.
#[derive(Clone, Copy)]
pub(crate) struct SymbolTableView<'a> {
    strings: StringHeapView<'a>,
    sorted: SortedView<'a>,
}

impl<'a> SymbolTableView<'a> {
    pub(crate) fn get(&self, sym: Sym) -> &'a str {
        self.strings.get(sym.as_u16())
    }

    /// Binary-search the permutation for `s`. `Ok(sym)` is the matching stable id; `Err(pos)`
    /// is the permutation slot a new symbol would take to keep it sorted (used by
    /// [`SymbolTable::try_get_or_insert`]). Byte-exact and case-sensitive — class matching
    /// never folds case.
    pub(crate) fn search(&self, s: &str) -> Result<Sym, usize> {
        use std::cmp::Ordering::*;
        let mut left = 0usize;
        let mut right = self.sorted.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let sym = self.sorted.at(mid);
            match self.strings.get(sym).cmp(s) {
                Less => left = mid + 1,
                Greater => right = mid,
                Equal => return Ok(Sym(sym)),
            }
        }
        Err(left)
    }

    /// Resolve `s` to its stable [`Sym`], or `None` if absent. Used by the selector resolve
    /// pass ([`crate::css::ClassSelector::resolve`]) to bind a class name to a document.
    pub(crate) fn find(&self, s: &str) -> Option<Sym> {
        self.search(s).ok()
    }
}

impl ArchivedSymbolTable {
    pub(crate) fn view(&self) -> SymbolTableView<'_> {
        SymbolTableView {
            strings: self.heap.view(),
            sorted: SortedView::Archived(self.sorted.as_slice()),
        }
    }
}
