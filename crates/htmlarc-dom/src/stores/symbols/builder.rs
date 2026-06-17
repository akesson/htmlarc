use crate::stores::interner::StringInterner;

use super::{LOCAL_CAP, Sym, SymbolTable};

/// Accumulates a document's symbols during parsing: a transient [`StringInterner`] dedups
/// each name into the heap exactly once, and [`build`](Self::build) sorts the content
/// permutation and drops the hash table (ADR 0002 §1). A capacity overflow is recorded as
/// a poison flag the parse path reads to discard the whole document (the PR 1 convention,
/// mirroring the former `ClassStoreBuilder`).
#[derive(Default)]
pub(crate) struct SymbolTableBuilder {
    interner: StringInterner,
    /// Set (first reason wins) once the per-document symbol ceiling is hit.
    overflow: Option<&'static str>,
}

impl SymbolTableBuilder {
    /// The reason this builder overflowed a per-document capacity, if any.
    pub(crate) fn overflow(&self) -> Option<&'static str> {
        self.overflow
    }

    /// Interns `s`, capped at [`LOCAL_CAP`]; on overflow it poisons the builder and
    /// returns `Sym(0)` (the document is discarded, so the bogus id is never observed).
    pub(crate) fn intern_or_poison(&mut self, s: &str) -> Sym {
        match self.interner.try_intern_capped(s, LOCAL_CAP) {
            Some(i) => Sym(i),
            None => {
                // The table holds class tokens, extended attribute names, and extended tag
                // names alike (ADR 0002 §3–§4), so the reason is phrased generically.
                self.overflow
                    .get_or_insert("identity strings exceed 61,184");
                Sym(0)
            }
        }
    }

    pub(crate) fn build(self) -> SymbolTable {
        // The heap stays in insertion order (each name copied once at parse); the content
        // permutation `find` binary-searches is computed in `from_interner`.
        SymbolTable::from_interner(self.interner)
    }
}
