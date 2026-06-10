use std::cell::RefCell;
use std::ops::RangeBounds;

use crate::dom::NodesView;
use crate::prelude::*;

use super::exactly_iter::Exactly;
use super::{DomIterator, VisitedStack, VisitedStatus};

/// The element iterator goes through the html elements in the same order
/// as they would be presented when rendered.
///
/// Before moving to next, it checks the current element to see if it was
/// deleted or if it's parent was changed (indication that it was moved).
/// If that is the case, it goes goes upwards in the hierarchy (stack) until it
/// finds an element that is not deleted with an unchanged parent and it continues
/// from there.
#[derive(Clone)]
pub struct ElementIter<'dom, Dom> {
    pub(super) dom: &'dom Dom,
    pub(super) stack: RefCell<VisitedStack>,
    include_comment: bool,
    include_text: bool,
}

impl<'dom, Dom: DomRead> ElementIter<'dom, Dom> {
    /// Iterates through all the descendants of the given element
    pub(crate) fn descendants<'a>(element: &'a HtmlElement<'dom, Dom>) -> Self {
        let HtmlElement { dom, index } = element;
        let stack = dom.with_view(|view| VisitedStack::from_element(view.nodes, *index));
        Self {
            dom,
            stack: stack.into(),
            include_comment: false,
            include_text: false,
        }
    }

    /// Starts at the current element and goes through all nodes until the end of
    /// the document, which means it will also include the parent's siblings.
    pub(crate) fn forwards<'a>(element: &'a HtmlElement<'dom, Dom>) -> Self {
        let HtmlElement { dom, index } = element;
        let stack = dom.with_view(|view| VisitedStack::from_root_to_element(view.nodes, *index));
        Self {
            dom,
            stack: stack.into(),
            include_comment: false,
            include_text: false,
        }
    }

    pub fn current(&self) -> Option<HtmlElement<'dom, Dom>> {
        self.stack
            .borrow()
            .last()
            .map(|index| HtmlElement::new(self.dom, index))
    }

    // /// Transform into a Reverse Element Iterator which goes through
    // /// the elements in an upwards (or backwards) order
    // pub fn rev(self) -> RevElementIter<'dom> {
    //     RevElementIter::reverse(&self.current().unwrap_or(Element::new(self.dom, 0)))
    // }

    pub fn exactly<R: RangeBounds<usize>>(self, range: R) -> Exactly<'dom, Dom, Self> {
        Exactly::new(self, range)
    }

    pub(super) fn find_next(&self, nodes: NodesView, go_deeper: bool) -> Option<NodeIndex> {
        // update the stack for any changes to the current (already visited) element.

        let index = match self.stack.borrow_mut().last_updated(nodes) {
            VisitedStatus::StackEmpty => return None,
            VisitedStatus::Changed(i) => return Some(i),
            VisitedStatus::Same(i) => i,
        };

        if go_deeper && let Some(child) = nodes.first_child_index(index) {
            self.stack.borrow_mut().push_first_child(child);
            return Some(child);
        }

        if self.stack.borrow().len() <= 1 {
            return None;
        }

        if let Some(sibling) = nodes.next_sibling_index(index) {
            self.stack.borrow_mut().set_next_sibling(sibling);
            return Some(sibling);
        }

        while let Some(index) = { self.stack.borrow().last() } {
            if self.stack.borrow().len() <= 1 {
                return None;
            }
            if let Some(sibling) = nodes.next_sibling_index(index) {
                self.stack.borrow_mut().set_next_sibling(sibling);
                return Some(sibling);
            }

            self.stack.borrow_mut().pop();
        }

        None
    }

    #[cfg(test)]
    fn string_take(&mut self, n: usize) -> String {
        self.take(n)
            .map(|el| format!("{} {}", el.tag(), el.index()))
            .collect::<Vec<String>>()
            .join(", ")
    }
    #[cfg(test)]
    fn string(self) -> String {
        self.map(|el| format!("{} {}", el.tag(), el.index()))
            .collect::<Vec<String>>()
            .join(", ")
    }
    #[cfg(test)]
    fn next_assert(&mut self, tag: HtmlTag) -> HtmlElement<'dom, Dom> {
        let el = self.next();
        assert_eq!(el.as_ref().map(|el| el.tag()), Some(tag));
        el.unwrap()
    }
}

impl<'dom, Dom: DomRead> Iterator for ElementIter<'dom, Dom> {
    type Item = HtmlElement<'dom, Dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_element()
    }
}

impl<'dom, Dom: DomRead> DomIterator<'dom, Dom> for ElementIter<'dom, Dom> {
    fn dom(&self) -> &'dom Dom {
        self.dom
    }

    fn next_index_and_depth(&self) -> Option<(NodeIndex, i16)> {
        let vals = self.dom.with_view(|view| self.find_next(view.nodes, true));
        let depth = self.stack.borrow().len() as i16;
        vals.map(|v| (v, depth))
    }

    fn set_include_comment(mut self) -> Self {
        self.include_comment = true;
        self
    }

    fn include_comment(&self) -> bool {
        self.include_comment
    }

    fn set_include_text(mut self) -> Self {
        self.include_text = true;
        self
    }

    fn include_text(&self) -> bool {
        self.include_text
    }
}

#[cfg(test)]
use crate::html::HtmlTag;

#[test]
fn root_descendants_iterator() {
    let html = parse("<body><div>hi</div></body>");
    let out = html.root().descendants().set_include_text().string();
    assert_eq!(out, "body 1, div 2, text 3");

    let html = parse("<body><div><span><i>test</i></span></div><p>some paragraph</p></body>");
    let out = html.root().descendants().set_include_text().string();
    assert_eq!(out, "body 1, div 2, span 3, i 4, text 5, p 6, text 7");
}

#[cfg(test)]
//  body 1
//      div 2
//          p 3
//              "hi" 4
//          dd 5
//              "ho" 6
//      section 7
const HTML1: &str = "<body><div><p>hi</p><dd>ho</dd></div><section></section></body>";

#[test]
fn descendants_iterator_plain() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::body);
    let out = el.descendants().set_include_text().string();
    assert_eq!(out, "div 2, p 3, text 4, dd 5, text 6, section 7");

    let html = parse(HTML1);

    let el = html.root();
    let el = el.first_child().unwrap();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::div);
    let out = el.descendants().set_include_text().string();
    assert_eq!(out, "p 3, text 4, dd 5, text 6");
}

#[test]
fn descendants_iterator_with_remove() {
    let html = parse(HTML1);

    let el = html.root();

    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::body);

    let mut it = el.descendants();
    let div = it.next_assert(HtmlTag::div);

    div.remove();

    assert_eq!(it.string(), "section 7");
}

#[test]
fn descendants_iterator_with_unwrap() {
    let html = parse(HTML1);

    let el = html.root();

    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::body);

    let mut it = el.descendants().set_include_text();
    let div = it.next_assert(HtmlTag::div);

    div.unwrap_element();

    assert_eq!(it.string(), "p 3, text 4, dd 5, text 6, section 7");
}

#[cfg(test)]
// body
//     div
//          "hi"
//     p
//          "ho"
//     footer
//          "ha"
const HTML2: &str = "<body><div>hi</div><p>ho</p><footer>ha</footer></body>";

#[test]
fn forward_iterator() {
    let html = parse(HTML2);
    let out = html.root().forwards().set_include_text().string();
    assert_eq!(out, "body 1, div 2, text 3, p 4, text 5, footer 6, text 7");

    let html = parse(HTML2);
    let el = html.root();
    let el = el.first_child().unwrap();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    assert_eq!(el.tag(), HtmlTag::p);

    assert_eq!(
        el.forwards().set_include_text().string(),
        "text 5, footer 6, text 7"
    );

    let html = parse(HTML2);
    let el = html.root();
    let el = el.first_child().unwrap();
    let el = el.first_child().unwrap();
    let el = el.first_child_all().unwrap();
    // on "hi" text node
    assert_eq!(el.tag(), HtmlTag::sys_text);

    assert_eq!(
        el.forwards().set_include_text().string(),
        "p 4, text 5, footer 6, text 7"
    );
}

#[test]
fn forward_iterator_with_remove() {
    let html = parse(HTML2);

    let el = html.root();
    let mut iter = el.forwards().set_include_text();

    assert_eq!(iter.string_take(2), "body 1, div 2");
    iter.current().unwrap().remove();
    assert_eq!(iter.string(), "p 4, text 5, footer 6, text 7");

    let html = parse(HTML2);
    let el = html.root();

    let mut iter = el.forwards().set_include_text();

    assert_eq!(iter.string_take(4), "body 1, div 2, text 3, p 4");

    // we remove the b tag
    assert_eq!(iter.current().map(|el| el.tag()), Some(HtmlTag::p));
    iter.current().unwrap().remove();

    assert_eq!(iter.string(), "footer 6, text 7");
}

#[ignore]
#[test]
fn forward_iterator_with_unwrap() {
    let html = parse(HTML2);
    let el = html.root();
    let mut iter = el.forwards();

    assert_eq!(iter.string_take(4), "body 1, div 2, text 3, p 4");

    // we unwrap the p tag
    iter.current().unwrap().unwrap_element();
    assert_eq!(iter.string(), "text 5, footer 6, text 7");
}

#[cfg(test)]
fn parse(s: &str) -> crate::dom::DomRefCell {
    crate::html::HtmlDoc::parse(s).unwrap().dom_ref_cell()
}

#[cfg(test)]
const HTML3: &str = r#"
<header>
    <h1></h1>
</header>
<main>
    <section>
        <p></p><!-- comment -->
        <article></article>
        <div>
            <span></span>
            <h2></h2>
            <!-- comment -->
            <p></p>
        </div>
        <a></a><!-- comment -->
        <h4></h4>
    </section>
</main>
"#;

#[test]
fn forward_iterator_toggle_text_and_comment() {
    let html = parse(HTML3);
    let el = html.root();

    let iter = el.forwards();
    assert_eq!(
        iter.string(),
        "header 2, h1 4, main 7, section 9, p 11, article 14, div 16, span 18, h2 20, p 24, a 27, h4 30"
    );

    let iter = el.forwards().set_include_text();
    assert_eq!(
        iter.string(),
        "text 1, header 2, text 3, h1 4, text 5, text 6, main 7, text 8, section 9, text 10, p 11, text 13, article 14, text 15, div 16, text 17, span 18, text 19, h2 20, text 21, text 23, p 24, text 25, text 26, a 27, text 29, h4 30, text 31, text 32, text 33"
    );

    let iter = el.forwards().set_include_comment();
    assert_eq!(
        iter.string(),
        "header 2, h1 4, main 7, section 9, p 11, comment 12, article 14, div 16, span 18, h2 20, comment 22, p 24, a 27, comment 28, h4 30"
    );

    let iter = el.forwards().set_include_comment().set_include_text();
    assert_eq!(
        iter.string(),
        "text 1, header 2, text 3, h1 4, text 5, text 6, main 7, text 8, section 9, text 10, p 11, comment 12, text 13, article 14, text 15, div 16, text 17, span 18, text 19, h2 20, text 21, comment 22, text 23, p 24, text 25, text 26, a 27, comment 28, text 29, h4 30, text 31, text 32, text 33"
    );
}
