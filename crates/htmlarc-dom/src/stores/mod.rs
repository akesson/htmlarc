mod attributes;
mod dataattributes;
mod interner;
mod listvec;
mod runs;
mod stringheap;
mod stringstack;
mod symbols;

pub use attributes::{Attribute, AttributeStore};
pub(crate) use attributes::{
    AttributeReBuilder, AttributeStoreBuilder, AttributeStoreView, attr_list,
};
pub use dataattributes::{DataAttribute, DataAttributeStore};
pub(crate) use dataattributes::{
    DataAttributeRebuilder, DataAttributeStoreBuilder, DataAttributeStoreView, data_attr_list,
};
// `ListVec` and its rebuilder stay internal to the attribute stores until ADR 0002 PR 3
// replaces them; only the list-slot index type remains part of their shared API.
pub use listvec::ListIndex;
// Class lists are contiguous runs of bare `Sym`s, so `dom`/`accessors` reach the run
// arena directly rather than through a class-specific store.
pub(crate) use runs::{RunIndex, RunRebuilder, RunValues, RunVec, RunVecView};
pub use stringstack::StringStack;
pub(crate) use stringstack::StringStackView;
pub use symbols::Class;
pub(crate) use symbols::{Sym, SymbolTable, SymbolTableBuilder, SymbolTableView};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ListRemovalResult {
    ListRemoved,
    EntryRemoved,
    NotFound,
}
