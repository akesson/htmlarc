mod accessors;
pub mod css;
mod dom;
mod error;
mod fmt;
mod html;
mod iters;
pub(crate) mod parser;
mod stores;

mod logging {
    #[cfg(not(test))]
    pub use log::debug;

    #[cfg(test)]
    pub use std::println as debug;
}

use error::Context;
pub use error::{ParseResult, SelectorError};
pub use logging::debug;

pub mod prelude {
    pub use crate::dom::{ArchivedDomInner, DomInner, DomOwn, DomRead, DomRef, DomRefCell, DomView};
    pub use crate::error::{ElementError, Locatable, LocatedError};
    pub use crate::fmt::HtmlFormat;
    pub use crate::here;
    pub use crate::html::{AssertElement, HtmlAttr, HtmlDoc, HtmlElement, HtmlTag};
    pub use crate::iters::{DomIterator, ElementIteration, Tag, TagIter};
    pub use crate::stores::{Attribute, Class, DataAttribute};
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum CodeLocation {
    Function {
        file: &'static str,
        function: &'static str,
        line: u32,
    },
    File(&'static str),
}

impl std::fmt::Display for CodeLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file = match self {
            CodeLocation::Function { file, .. } | CodeLocation::File(file) => file,
        };
        if let Some((crate_name, path)) = file_to_crate_and_path(file) {
            write!(f, "{}::{}", crate_name, path)?;
        } else {
            write!(f, "{}", file)?;
        }
        if let CodeLocation::Function { function, line, .. } = self {
            write!(f, ":{line} fn {function}")?;
        }
        Ok(())
    }
}

fn file_to_crate_and_path(file: &'static str) -> Option<(&'static str, &'static str)> {
    let (before_src, path) = file.split_once("/src/")?;
    let crate_name = before_src.split('/').next_back()?;
    Some((crate_name, path))
}

/// IMPORTANT: You have to annotate the function with `#[named]` to use this macro
#[macro_export]
macro_rules! here {
    () => {
        $crate::CodeLocation::Function {
            file: file!(),
            function: function_name!(),
            line: line!(),
        }
    };
}

#[test]
#[function_name::named]
fn location_test() {
    use std::panic::Location;
    assert_eq!(
        here!().to_string(),
        "htmlarc-dom::lib.rs:83 fn location_test"
    );
    assert_eq!(
        CodeLocation::File(Location::caller().file()).to_string(),
        "htmlarc-dom::lib.rs"
    );
}
