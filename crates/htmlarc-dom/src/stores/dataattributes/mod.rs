mod lists;
mod rebuilder;
mod store;

use std::fmt::Display;

pub(crate) use lists::{DataAttributeList, data_attr_list};
pub(crate) use rebuilder::DataAttributeRebuilder;
pub use store::DataAttributeStore;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DataAttribute<'a> {
    pub tag: &'a str,
    pub val: &'a str,
}

impl Display for DataAttribute<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "data-{}='{}'", self.tag, self.val)
    }
}
