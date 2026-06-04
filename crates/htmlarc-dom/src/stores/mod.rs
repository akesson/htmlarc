mod attributes;
mod classes;
mod dataattributes;
mod listvec;
mod stringheap;
mod stringstack;

pub use attributes::{Attribute, AttributeStore};
pub(crate) use attributes::{AttributeList, AttributeReBuilder, AttributeStoreBuilder, attr_list};
pub use classes::{Class, ClassStore};
pub(crate) use classes::{ClassList, ClassReBuilder, ClassStoreBuilder, class_list};
pub use dataattributes::{DataAttribute, DataAttributeStore};
pub(crate) use dataattributes::{DataAttributeList, DataAttributeRebuilder, data_attr_list};
pub use listvec::ListIndex;
pub use stringstack::StringStack;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ListRemovalResult {
    ListRemoved,
    EntryRemoved,
    NotFound,
}
