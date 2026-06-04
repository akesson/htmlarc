#[cfg(test)]
mod tests;

mod builder;
mod lists;
mod rebuilder;
mod store;

use std::fmt::Display;

use crate::html::HtmlAttr;

pub(crate) use builder::AttributeStoreBuilder;
pub(crate) use lists::{AttributeList, attr_list};
pub(crate) use rebuilder::AttributeReBuilder;
pub use store::AttributeStore;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Attribute<'a> {
    pub tag: HtmlAttr,
    pub val: &'a str,
}

impl Display for Attribute<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.val.is_empty() {
            write!(f, "{}", self.tag)
        } else {
            write!(f, "{}=\"{}\"", self.tag, self.val)
        }
    }
}
