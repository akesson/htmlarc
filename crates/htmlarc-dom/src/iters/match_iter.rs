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
    pub fn new(iter: I, selectors: SelectorList<'dom>) -> Self {
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
