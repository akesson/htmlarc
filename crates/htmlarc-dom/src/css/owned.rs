//! A compiled selector that owns its source, decoupling the selector's lifetime from both the
//! source string and any document.

use crate::css::{ParseError, parse_css, selectors::SelectorList};

self_cell::self_cell!(
    /// A compiled CSS selector list that owns its source string — the `'static` counterpart of
    /// [`SelectorList`], whose borrows tie it to the source `&str` (and, through
    /// [`select`](crate::html::HtmlElement::select), to the document's lifetime).
    ///
    /// Compile once with [`parse`](OwnedSelectorList::parse), then run against any number of
    /// documents — owned, memory-mapped, or on other threads — via
    /// [`list`](OwnedSelectorList::list):
    ///
    /// ```
    /// use htmlarc_dom::css::OwnedSelectorList;
    /// use htmlarc_dom::prelude::*;
    ///
    /// let selector = OwnedSelectorList::parse(".title")?;
    /// let doc = HtmlDoc::parse(r#"<body><h1 class="title">hi</h1></body>"#)
    ///     .unwrap()
    ///     .dom();
    /// let hits: Vec<_> = doc.root().select(selector.list().clone()).collect();
    /// assert_eq!(hits.len(), 1);
    /// # Ok::<(), htmlarc_dom::css::ParseError>(())
    /// ```
    pub struct OwnedSelectorList {
        owner: String,

        #[covariant]
        dependent: SelectorList,
    }
);

impl OwnedSelectorList {
    /// Compile `source` into an owned selector list. The parse is exactly
    /// [`parse_css`](crate::css::parse_css); only the ownership differs.
    pub fn parse(source: impl Into<String>) -> Result<Self, ParseError> {
        Self::try_new(source.into(), |s| parse_css(s))
    }

    /// The compiled list, borrowed at the handle's lifetime. [`SelectorList`] is covariant in its
    /// source lifetime, so this passes anywhere a shorter-lived list is expected — clone it into
    /// [`select`](crate::html::HtmlElement::select), which takes the list by value.
    pub fn list(&self) -> &SelectorList<'_> {
        self.borrow_dependent()
    }

    /// The selector source string this handle owns.
    pub fn source(&self) -> &str {
        self.borrow_owner()
    }
}

impl std::fmt::Debug for OwnedSelectorList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OwnedSelectorList")
            .field(&self.source())
            .finish()
    }
}

impl std::fmt::Display for OwnedSelectorList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.list().fmt(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn owned_selector_is_static_send_sync() {
        fn assert_handle<T: Send + Sync + 'static>() {}
        assert_handle::<OwnedSelectorList>();
    }

    #[test]
    fn parse_error_surfaces() {
        assert!(OwnedSelectorList::parse("div,").is_err());
    }

    #[test]
    fn one_compile_runs_against_many_docs_and_threads() {
        let selector = OwnedSelectorList::parse("p.x, h1").unwrap();
        assert_eq!(selector.source(), "p.x, h1");

        let doc_a = HtmlDoc::parse(r#"<body><p class="x">a</p><p>skip</p></body>"#)
            .unwrap()
            .dom();
        let doc_b = HtmlDoc::parse(r#"<body><h1>b</h1></body>"#).unwrap().dom();

        let hits = |dom: &DomInner| -> Vec<HtmlTag> {
            dom.root()
                .select(selector.list().clone())
                .map(|el| el.tag())
                .collect()
        };
        assert_eq!(hits(&doc_a), vec![HtmlTag::p]);
        assert_eq!(hits(&doc_b), vec![HtmlTag::h1]);

        // The compiled selector moves across threads with its source.
        let joined = std::thread::spawn(move || {
            let doc = HtmlDoc::parse(r#"<body><h1>c</h1></body>"#).unwrap().dom();
            doc.root().select(selector.list().clone()).count()
        })
        .join()
        .unwrap();
        assert_eq!(joined, 1);
    }
}
