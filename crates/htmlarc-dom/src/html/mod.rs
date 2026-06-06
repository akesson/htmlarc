mod doc;
mod element;
mod html_attr;
mod html_tag;
#[cfg(test)]
mod tests;

pub use doc::HtmlDoc;
pub use element::{ElementType, HtmlElement};
pub(crate) use element::IGNORE_TAGS;
pub use html_attr::HtmlAttr;
pub use html_tag::HtmlTag;

#[cfg(test)]
pub(crate) use assert_element::AssertElement;

/// Test-only assertion helper: unwrap an element result and assert its tag.
///
/// Gated to `cfg(test)` — it panics on `None`/`Err`, so it must not sit in the
/// public API surface.
#[cfg(test)]
mod assert_element {
    use super::{HtmlElement, HtmlTag};
    use crate::dom::DomRead;

    pub trait AssertElement<'dom, Dom> {
        fn assert(self, tag: HtmlTag) -> HtmlElement<'dom, Dom>;
    }

    impl<'dom, Dom: DomRead> AssertElement<'dom, Dom> for Option<HtmlElement<'dom, Dom>> {
        fn assert(self, tag: HtmlTag) -> HtmlElement<'dom, Dom> {
            let el = self.expect("Expected element to be some");
            assert_eq!(el.tag(), tag);
            el
        }
    }

    impl<'dom, Dom: DomRead, Err: std::fmt::Debug> AssertElement<'dom, Dom>
        for Result<HtmlElement<'dom, Dom>, Err>
    {
        fn assert(self, tag: HtmlTag) -> HtmlElement<'dom, Dom> {
            let el = self.expect("Expected element to be Ok");
            assert_eq!(el.tag(), tag);
            el
        }
    }
}
