mod chars_iter;
mod element_iter;
mod element_iter_rev;
mod exactly_iter;
mod linear_iter;
mod match_iter;
mod node_chars;
mod relative_iter;
mod stack;
mod tag_iter;

use std::ops::RangeBounds;

use crate::{
    css::SelectorList,
    dom::{DomRead, NodeIndex},
    error::IterationError,
    html::{HtmlElement, HtmlTag},
};
pub use chars_iter::CharsIter;
pub use element_iter::ElementIter;
pub use element_iter_rev::RevElementIter;
use exactly_iter::Exactly;
pub use linear_iter::LinearSweep;
pub use match_iter::MatchIter;
pub use relative_iter::RelativeIter;
use stack::{VisitedStack, VisitedStatus};
pub use tag_iter::{Tag, TagIter};

const DBG: bool = false;
const MAX_DEPTH: usize = 32;

pub trait DomIterator<'dom, Dom: DomRead + 'dom>: Iterator<Item = HtmlElement<'dom, Dom>> {
    fn dom(&self) -> &'dom Dom;
    /// returns the next element index if any
    fn next_index(&self) -> Option<NodeIndex>;
    fn set_include_comment(self) -> Self;
    fn include_comment(&self) -> bool;
    fn set_include_text(self) -> Self;
    fn include_text(&self) -> bool;
    fn next_element(&self) -> Option<HtmlElement<'dom, Dom>> {
        loop {
            let index = self.next_index()?;
            let el = HtmlElement::new(self.dom(), index);
            let tag = el.tag();

            if tag == HtmlTag::sys_text && !self.include_text() {
                continue;
            }
            if tag == HtmlTag::sys_comment && !self.include_comment() {
                continue;
            }

            return Some(el);
        }
    }
    fn text_chars(self) -> CharsIter<'dom, Dom, Self>
    where
        Self: Iterator<Item = HtmlElement<'dom, Dom>> + Sized,
    {
        CharsIter::forward(self.set_include_text())
    }
    fn comment_chars(self) -> CharsIter<'dom, Dom, Self>
    where
        Self: Iterator<Item = HtmlElement<'dom, Dom>> + Sized,
    {
        CharsIter::forward(self.set_include_comment()).include_comments()
    }
    fn text_and_comment_chars(self) -> CharsIter<'dom, Dom, Self>
    where
        Self: Iterator<Item = HtmlElement<'dom, Dom>> + Sized,
    {
        CharsIter::forward(self.set_include_text().set_include_comment()).include_comments()
    }
    fn select(self, selectors: SelectorList<'dom>) -> MatchIter<'dom, Dom, Self>
    where
        Self: Sized,
    {
        MatchIter::new(self, selectors)
    }
    /// Wrap this walk in an [`Exactly`] cardinality check over `range`. Provided on the trait so
    /// every `DomIterator` (tree-walk or [`LinearSweep`]) shares one implementation. (`MatchIter`
    /// is `Iterator`-only, not a `DomIterator`, so it keeps its own inherent `exactly`.)
    fn exactly<R: RangeBounds<usize>>(self, range: R) -> Exactly<'dom, Dom, Self>
    where
        Self: Sized,
    {
        Exactly::new(self, range)
    }
}

pub trait ElementIteration<'dom, Dom> {
    fn first(&mut self) -> Result<HtmlElement<'dom, Dom>, IterationError>;
}

impl<'dom, Dom: 'dom, T> ElementIteration<'dom, Dom> for T
where
    T: Iterator<Item = HtmlElement<'dom, Dom>>,
{
    fn first(&mut self) -> Result<HtmlElement<'dom, Dom>, IterationError> {
        self.next().ok_or(IterationError::Empty)
    }
}
