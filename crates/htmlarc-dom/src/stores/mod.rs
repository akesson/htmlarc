mod attributes;
mod dataattributes;
mod interner;
mod listvec;
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
pub use listvec::ListIndex;
// Class lists hold bare `Sym`s now, so `dom`/`accessors` reach the list machinery directly
// rather than through a class-specific store.
pub(crate) use listvec::{ListRebuilder, ListVec, ListVecView};
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
