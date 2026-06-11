mod attributes;
mod classes;
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
pub use classes::{Class, ClassStore};
pub(crate) use classes::{
    ClassList, ClassReBuilder, ClassStoreBuilder, ClassStoreView, class_list,
};
pub use dataattributes::{DataAttribute, DataAttributeStore};
pub(crate) use dataattributes::{
    DataAttributeRebuilder, DataAttributeStoreBuilder, DataAttributeStoreView, data_attr_list,
};
pub use listvec::ListIndex;
pub use stringstack::StringStack;
pub(crate) use stringstack::StringStackView;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ListRemovalResult {
    ListRemoved,
    EntryRemoved,
    NotFound,
}
