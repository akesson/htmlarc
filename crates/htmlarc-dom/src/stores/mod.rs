mod attrstore;
mod ext_tags;
mod interner;
mod runs;
mod string_source;
mod stringheap;
mod stringstack;
mod symbols;

// One unified attribute store (ADR 0002 §3): standard, `data-*`, and unknown attributes
// alike, with per-element attribute lists held as contiguous entry-id runs.
pub use attrstore::{AttrName, AttrStore, Attribute};
pub(crate) use attrstore::{AttrStoreBuilder, AttrStoreView, NAME_EXT_BASE};
// The per-document extended-tag vocab (ADR 0002 §4): unknown/custom tag names live as
// symbols, encoded into the node tag byte's `[EXT_BASE, 255]` range.
pub(crate) use ext_tags::{EXT_BASE, EXT_OVERFLOW, ExtTags, ExtTagsView};
// Class and attribute lists are contiguous runs of bare ids, so `dom`/`accessors` reach the
// run arena directly rather than through a list-specific store.
pub(crate) use runs::{RunIndex, RunRebuilder, RunValues, RunVec, RunVecView};
pub use string_source::StringSource;
pub use stringstack::StringStack;
pub use symbols::Class;
pub(crate) use symbols::{Sym, SymbolTable, SymbolTableBuilder, SymbolTableView};
