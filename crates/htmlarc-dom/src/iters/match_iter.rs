use std::ops::RangeBounds;

use crate::{
    css::{Selector, SelectorList},
    prelude::*,
};

use super::{DomIterator, exactly_iter::Exactly};

pub struct MatchIter<'dom, Dom, I>
where
    I: Iterator<Item = HtmlElement<'dom, Dom>>,
    Self: 'dom,
{
    iter: I,
    selectors: SelectorList<'dom>,
}

impl<'dom, Dom, I> MatchIter<'dom, Dom, I>
where
    Dom: DomRead,
    I: Iterator<Item = HtmlElement<'dom, Dom>> + DomIterator<'dom, Dom>,
    Self: 'dom,
{
    pub fn new(iter: I, mut selectors: SelectorList<'dom>) -> Self {
        // Bind the owned selector list to this document once: every class selector (incl.
        // those nested in :not/:is/:has) resolves to a Sym or Absent, so per-node matching
        // is integer compares (ADR 0002 §3). filter.rs clones the list per document, so this
        // only ever mutates a per-document copy.
        iter.dom().with_view(|view| selectors.resolve(view.symbols));
        Self { iter, selectors }
    }

    pub fn exactly<R: RangeBounds<usize>>(self, range: R) -> Exactly<'dom, Dom, Self> {
        Exactly::new(self, range)
    }
}

impl<'dom, Dom, I> Iterator for MatchIter<'dom, Dom, I>
where
    Dom: DomRead,
    I: Iterator<Item = HtmlElement<'dom, Dom>> + DomIterator<'dom, Dom>,
    Self: 'dom,
{
    type Item = HtmlElement<'dom, Dom>;

    fn next(&mut self) -> Option<Self::Item> {
        let dom = self.iter.dom();
        while let Some((el_index, _)) = self.iter.next_index_and_depth() {
            let element = HtmlElement::new(dom, el_index);
            if element.tag() == HtmlTag::sys_text {
                continue;
            }
            if self.selectors.matches(&element) {
                return Some(element);
            }
        }
        None
    }
}

#[test]
fn test_single_div() {
    let html = "<body><div>hi</div></body>";
    assert_eq!(find(html, "div"), "div 2")
}

#[test]
fn test_three_divs() {
    // - body
    //    - div - div - "hi"
    //    - div
    let html = "<body><div><div>hi</div></div><div/></body>";
    assert_eq!(find(html, "div"), "div 2, div 3, div 5")
}

#[test]
fn test_div_after_section() {
    // - body
    //    - section - div - div - "hi"
    //    - div
    let html = "<body><section><div><div>hi</div></div></section></body>";
    assert_eq!(find(html, "section > div"), "div 3")
}

#[test]
fn test_div_after_div() {
    // - body
    //    - div - div - div - "hi"
    //    - div
    let html = "<body><div><div><div>hi</div></div></div><div/></body>";
    assert_eq!(find(html, "div > div"), "div 3, div 4")
}

#[test]
fn test_div_or_a_after_section() {
    // - body
    //    - section
    //          - div - b
    //          - span
    //          - a
    //    - div
    let html = "<body><section><div><b/></div><span/><a/></section><div/></body>";
    assert_eq!(find(html, "section > div, a"), "div 3, a 6")
}

#[test]
fn test_body_with_descendant_section() {
    // - body
    //    - div
    //          - section
    //               - div
    //    - span
    //          - p
    //               - section
    let html =
        "<body><div><section><div/></section></div><span><p><section></section></p></span></body>";
    assert_eq!(find(html, "body section"), "section 3, section 7")
}

#[cfg(test)]
fn find(html: &str, css: &str) -> String {
    let doc = HtmlDoc::parse(html).unwrap();
    let dom = doc.dom();
    let root = dom.root();
    root.select_css(css)
        .unwrap()
        .map(|el| format!("{} {}", el.tag(), el.index))
        .collect::<Vec<_>>()
        .join(", ")
}

// --- resolve-once class matching (ADR 0002 §3) ---
//
// `select_css` resolves every class selector against the document's symbol table before
// walking, so a present class matches by Sym, an absent one never matches, and `:not`
// negates that correctly. The cases below exercise each path through `MatchIter::new`.

#[test]
fn resolve_present_class_matches() {
    let html = r#"<body><div class="a b"></div><span class="c"></span></body>"#;
    assert_eq!(find(html, ".a"), "div 2");
    assert_eq!(find(html, ".b"), "div 2");
    // multi-class compound: both tokens must be present on the node
    assert_eq!(find(html, ".a.b"), "div 2");
    assert_eq!(find(html, ".c"), "span 3");
}

#[test]
fn resolve_absent_class_matches_nothing() {
    let html = r#"<body><div class="a"></div><span class="b"></span></body>"#;
    // ".absent" is not in the document's symbol table → resolves to Absent → no match.
    assert_eq!(find(html, ".absent"), "");
    // one absent token fails the whole compound even when the other is present.
    assert_eq!(find(html, ".a.absent"), "");
}

#[test]
fn resolve_not_absent_matches_all_divs() {
    let html = r#"<body><div class="x"></div><div></div><div class="y"></div></body>"#;
    // :not(.absent) — the Absent inner never matches, so the negation matches every div.
    assert_eq!(find(html, "div:not(.absent)"), "div 2, div 3, div 4");
    // :not(.x) — resolves .x to a Sym, excluding only the div that carries it.
    assert_eq!(find(html, "div:not(.x)"), "div 3, div 4");
}

#[test]
fn resolve_recurses_through_is_and_has() {
    let html = r#"<body><p class="a"></p><p class="b"></p></body>"#;
    // :is(...) resolves both inner class selectors; only .a is present here.
    assert_eq!(find(html, "p:is(.a, .absent)"), "p 2");

    let nested = r#"<body><div><span class="child"></span></div><div><span class="other"></span></div></body>"#;
    // :has(...) resolves through the relative selector list.
    assert_eq!(find(nested, "div:has(.child)"), "div 2");
    assert_eq!(find(nested, "div:has(.absent)"), "");
}

#[test]
fn direct_matches_uses_string_fallback() {
    // Element::matches_css does NOT run the resolve pass, so its class selectors stay
    // Unresolved and match by string comparison — the public direct-match path still works.
    let html = r#"<body><div class="hello world"></div></body>"#;
    let doc = HtmlDoc::parse(html).unwrap();
    let dom = doc.dom();
    let div = dom
        .root()
        .forwards()
        .find(|e| e.tag() == HtmlTag::div)
        .unwrap();
    assert!(div.matches_css(".hello").unwrap());
    assert!(div.matches_css(".hello.world").unwrap());
    assert!(!div.matches_css(".absent").unwrap());
}
