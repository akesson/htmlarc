pub mod accessors;
pub mod css;
pub mod dom;
pub(crate) mod entities;
pub mod error;
pub mod fmt;
pub mod html;
pub mod iters;
pub(crate) mod parser;
// `stores` is internal plumbing; its only user-facing handles (`Attribute`,
// `Class`, `DataAttribute`) are re-exported through the prelude.
mod stores;

mod logging {
    #[cfg(not(test))]
    pub use log::debug;

    #[cfg(test)]
    pub use std::println as debug;
}

pub use error::{HtmlParseError, HtmlParseResult};
pub use logging::debug;

pub mod prelude {
    //! Convenience glob: everything needed to query a DOM, including the concrete
    //! iterator/accessor/error types that the [`HtmlElement`](crate::html::HtmlElement)
    //! methods return — so callers can name what the API hands back. The same items
    //! are also reachable through their domain modules (`crate::iters`, `crate::css`, …).
    pub use crate::accessors::{Attributes, AttributesMut, Classes, ClassesMut};
    pub use crate::css::{AttributeSelector, ParseError, Selector, SelectorList, parse_css};
    pub use crate::dom::{
        ArchivedDom, ArchivedDomInner, DomInner, DomRead, DomRef, DomRefCell, DomView, NodeIndex,
    };
    pub use crate::error::{
        ElementError, HtmlParseError, HtmlParseResult, IterationError, Locatable, LocatedError,
    };
    pub use crate::fmt::HtmlFormat;
    pub use crate::here;
    pub use crate::html::{HtmlAttr, HtmlDoc, HtmlElement, HtmlTag};
    pub use crate::iters::{
        CharsIter, DomIterator, ElementIter, ElementIteration, MatchIter, RelativeIter,
        RevElementIter, Tag, TagIter,
    };
    pub use crate::stores::{AttrName, Attribute, Class, FrameDecoder, LazyState, StringSource};
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
        "htmlarc-dom::lib.rs:98 fn location_test"
    );
    assert_eq!(
        CodeLocation::File(Location::caller().file()).to_string(),
        "htmlarc-dom::lib.rs"
    );
}
