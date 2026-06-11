mod attrstore;
mod interner;
mod runs;
mod stringheap;
mod stringstack;
mod symbols;

// One unified attribute store (ADR 0002 §3): standard, `data-*`, and unknown attributes
// alike, with per-element attribute lists held as contiguous entry-id runs.
pub use attrstore::{AttrName, AttrStore, Attribute};
pub(crate) use attrstore::{AttrStoreBuilder, AttrStoreView, NAME_EXT_BASE};
// Class and attribute lists are contiguous runs of bare ids, so `dom`/`accessors` reach the
// run arena directly rather than through a list-specific store.
pub(crate) use runs::{RunIndex, RunRebuilder, RunValues, RunVec, RunVecView};
pub use stringstack::StringStack;
pub(crate) use stringstack::StringStackView;
pub use symbols::Class;
pub(crate) use symbols::{Sym, SymbolTable, SymbolTableBuilder, SymbolTableView};
