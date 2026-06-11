//! ADR 0002 §1–§2 — the per-document symbol table and its reference-space constants.
//!
//! A [`SymbolTable`] deduplicates a document's identity strings (class tokens today;
//! extended tag/attr names and Lane A attribute values in later PRs) and hands out a
//! stable [`Sym`] per distinct string. Ids are insertion-ordered heap indices and never
//! change; the sort order lives in a separate permutation, so symbols compare as plain
//! integers and live inserts cost one memmove instead of a value reindex.
//
// The whole module is wired into `DomInner`, the parser, and the selector engine in the
// commit that follows; until then its items (and the re-exports below) have no caller in
// the non-test library build, so dead-code / unused-import analysis would fire. This
// blanket allow is removed in that commit, when everything gains a real caller.
#![allow(dead_code, unused_imports)]

#[cfg(test)]
mod tests;

mod builder;
mod table;

use std::fmt::Display;

pub(crate) use builder::SymbolTableBuilder;
pub(crate) use table::{SymbolTable, SymbolTableView};

/// Provisional reference-space constants (ADR 0002 §2), frozen at the last format-touching
/// PR. Scope-ordered: document-local `[0, LOCAL_CAP)`, then a gap reserved for the
/// `+256`-biased name space (PR 3), then the bundle-shared range `[SHARED_BASE, NONE)`
/// (reserved but dormant until the bundle Lane A lands), then the universal `NONE`
/// sentinel. `NONE` never collides with a real [`Sym`] because every `Sym < LOCAL_CAP`; it
/// also coincides in value with the `ListVec` empty-head marker and the node slot's "no
/// list" sentinel, which is safe for exactly the same reason.
pub(crate) const LOCAL_CAP: u16 = 0xEF00; // 61,184 — per-document symbol ceiling
pub(crate) const SHARED_BASE: u16 = 0xF000; // start of the dormant bundle-shared range
pub(crate) const NONE: u16 = 0xFFFF;

/// A stable, insertion-ordered id of a deduplicated identity string in a document's
/// [`SymbolTable`]. Ids never change after insertion; the table's separate permutation
/// carries the sort order, so `Sym`s are compared as plain integers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Sym(pub(crate) u16);

impl Sym {
    pub(crate) fn as_u16(self) -> u16 {
        self.0
    }
}

impl From<u16> for Sym {
    fn from(value: u16) -> Self {
        Self(value)
    }
}

impl Display for Sym {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
