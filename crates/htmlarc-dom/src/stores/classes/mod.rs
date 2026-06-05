#[cfg(test)]
mod tests;

mod builder;
mod lists;
mod rebuilder;
mod store;

use std::fmt::Display;

pub(crate) use builder::ClassStoreBuilder;
pub(crate) use lists::{ClassList, class_list};
pub(crate) use rebuilder::ClassReBuilder;
pub use store::ClassStore;
pub(crate) use store::ClassStoreView;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Class<'a>(pub(crate) &'a str);

impl<'a> From<&'a str> for Class<'a> {
    fn from(value: &'a str) -> Self {
        Self(value)
    }
}

impl Display for Class<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
