mod attribute;
mod class;
mod complex;
mod complex_relative;
mod compound;
mod id;
mod list;
mod list_relative;
mod pseudo_class;
mod relative;
mod tag;

pub use attribute::{AttributeSelector, AttributeSelectorError};
pub(crate) use class::ResolvedSym;
pub use class::{ClassSelector, ClassSelectorError};
pub use complex::{ComplexSelector, ComplexSelectorError};
pub use complex_relative::{ComplexRelativeSelector, ComplexRelativeSelectorError};
pub use compound::{CompoundSelector, CompoundSelectorError};
pub(crate) use id::ResolvedRef;
pub use id::{IdSelector, IdSelectorError};
pub use list::{SelectorList, SelectorListError};
pub use list_relative::{RelativeSelectorList, RelativeSelectorListError};
pub use pseudo_class::{PseudoClassSelector, PseudoClassSelectorError};
pub use relative::{RelativeSelector, RelativeSelectorError};
pub use tag::TagSelector;

use crate::{dom::DomRead, html::HtmlElement};

use super::CssPattern;

pub trait Selector<'s>: CssPattern<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool;
}
