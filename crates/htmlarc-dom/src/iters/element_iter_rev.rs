use std::cell::Cell;

use crate::dom::NodesView;
use crate::prelude::*;

use super::{CharsIter, DomIterator, stack::SimpleStack};

#[derive(Clone, Copy, PartialEq)]
enum NodeOperation {
    IncludeStart,
    ExcludeStart,
    CheckChildren,
}

/// The element iterator goes through the html elements in the same order
/// as they would be presented when rendered, but backwards (or upwards).
#[derive(Clone)]
pub struct RevElementIter<'dom, Dom> {
    pub(super) dom: &'dom Dom,
    stack: SimpleStack,
    operation: Cell<NodeOperation>,
    pub(super) current_index: Cell<Option<NodeIndex>>,
    include_comment: bool,
    include_text: bool,
}

impl<'dom, Dom: DomRead> RevElementIter<'dom, Dom> {
    pub(crate) fn reverse(element: &HtmlElement<'dom, Dom>) -> Self {
        let HtmlElement { dom, index } = element;
        let stack = dom.with_nodes(|nodes| SimpleStack::from_root_to_element(nodes, *index));

        Self {
            dom: *dom,
            stack,
            operation: Cell::new(NodeOperation::ExcludeStart),
            current_index: Cell::new((*index).into()),
            include_comment: false,
            include_text: false,
        }
    }

    pub fn include_start(self) -> Self {
        self.operation.set(NodeOperation::IncludeStart);
        self
    }

    fn next_dom_inner(&self, nodes: NodesView) -> Option<NodeIndex> {
        if self.operation.get() == NodeOperation::ExcludeStart {
            // we remove the excluded item
            let Some(index) = self.stack.pop() else {
                self.current_index.set(None);
                return None;
            };

            if let Some(mut prev_sibling) = nodes.prev_sibling_index(index) {
                // we add the previous sibling to the stack
                self.stack.push(prev_sibling);

                // now we have to check if the previous sibling has children
                while let Some(last_child) = nodes.last_child_index(prev_sibling) {
                    // add every last child of the current item to the stack
                    self.stack.push(last_child);
                    prev_sibling = last_child;
                }
            }
        } else if self.operation.get() == NodeOperation::CheckChildren {
            let Some(mut index) = self.stack.last() else {
                self.current_index.set(None);
                return None;
            };

            while let Some(last_child) = nodes.last_child_index(index) {
                // add every last child of the current item to the stack
                self.stack.push(last_child);
                index = last_child;
            }
        }

        if self.stack.len() <= 1 {
            // we are at the root
            self.current_index.set(None);
            return None;
        }

        // now the index should point at the inner most last child or the parent
        let Some(index) = self.stack.pop() else {
            self.current_index.set(None);
            return None;
        };

        if let Some(prev_sibling) = nodes.prev_sibling_index(index) {
            // if the current item has a previous sibling, add it to the stack
            self.stack.push(prev_sibling);
            // and set the next operation to check its children
            self.operation.set(NodeOperation::CheckChildren);
        } else {
            // otherwise we set the next operation to include the start of the next item
            self.operation.set(NodeOperation::IncludeStart);
        }

        self.current_index.set(Some(index));

        Some(index)
    }

    #[cfg(test)]
    fn string(self) -> String {
        self.map(|el| format!("{} {}", el.tag(), el.index()))
            .collect::<Vec<String>>()
            .join(", ")
    }
}

impl<'dom, Dom: DomRead> Iterator for RevElementIter<'dom, Dom> {
    type Item = HtmlElement<'dom, Dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_element()
    }
}
impl<'dom, Dom: DomRead> DomIterator<'dom, Dom> for RevElementIter<'dom, Dom> {
    fn dom(&self) -> &'dom Dom {
        self.dom
    }

    fn next_index(&self) -> Option<NodeIndex> {
        self.dom().with_nodes(|nodes| self.next_dom_inner(nodes))
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

    fn text_chars(self) -> CharsIter<'dom, Dom, Self> {
        CharsIter::reverse(self.set_include_text())
    }
    fn comment_chars(self) -> CharsIter<'dom, Dom, Self> {
        CharsIter::reverse(self.set_include_comment()).include_comments()
    }
    fn text_and_comment_chars(self) -> CharsIter<'dom, Dom, Self> {
        CharsIter::reverse(self.set_include_text().set_include_comment()).include_comments()
    }
}

#[cfg(test)]
use crate::html::HtmlTag;

#[cfg(test)]
// head 1
// body 2
//   div 3
//     b 4
//       text 5
//   article 6
//     h1 7
//       text 8
//     p 9
//       text 10
//   section 11
const HTML1: &str = "<head></head><body><div><b>text</b></div><article><h1>text</h1><p>text</p></article><section></section></body>";

#[test]
fn rev_iter_no_start_from_last_element() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();
    assert_eq!(el.tag(), HtmlTag::section);
    let out = RevElementIter::reverse(&el).set_include_text().string();
    assert_eq!(
        out, "text 10, p 9, text 8, h1 7, article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the last item of the html without including it"
    );
}

#[test]
fn rev_iter_no_start_from_last_child() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    let el = el.last_child_all().unwrap();
    assert_eq!(el.tag(), HtmlTag::p);
    let out = RevElementIter::reverse(&el).set_include_text().string();
    assert_eq!(
        out, "text 8, h1 7, article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the last child of an element without including it"
    );
}

#[test]
fn rev_iter_no_start_from_first_child() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::h1);
    let out = RevElementIter::reverse(&el).set_include_text().string();
    assert_eq!(
        out, "article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the first child of an element without including it"
    );
}

#[test]
fn rev_iter_no_start_from_first_element() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::head);
    let out = RevElementIter::reverse(&el).string();
    assert_eq!(
        out, "",
        "Should return nothing when reversing from the first element without including it"
    );
}

#[test]
fn rev_iter_start_from_last_element() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();
    assert_eq!(el.tag(), HtmlTag::section);
    let out = RevElementIter::reverse(&el)
        .include_start()
        .set_include_text()
        .string();
    assert_eq!(
        out,
        "section 11, text 10, p 9, text 8, h1 7, article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the last item of the html"
    );
}

#[test]
fn rev_iter_start_from_last_child() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    let el = el.last_child_all().unwrap();
    assert_eq!(el.tag(), HtmlTag::p);
    let out = RevElementIter::reverse(&el)
        .include_start()
        .set_include_text()
        .string();
    assert_eq!(
        out, "p 9, text 8, h1 7, article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the last child of an element"
    );
}

#[test]
fn rev_iter_start_from_first_child() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::h1);
    let out = RevElementIter::reverse(&el)
        .include_start()
        .set_include_text()
        .string();
    assert_eq!(
        out, "h1 7, article 6, text 5, b 4, div 3, body 2, head 1",
        "Should reverse iterate starting from the first child of an element"
    );
}

#[test]
fn rev_iter_start_from_first_element() {
    let html = parse(HTML1);

    let el = html.root();
    let el = el.first_child().unwrap();
    assert_eq!(el.tag(), HtmlTag::head);
    let out = RevElementIter::reverse(&el).include_start().string();
    assert_eq!(out, "head 1", "Should reverse from the first element");
}

#[cfg(test)]
fn parse(s: &str) -> DomInner {
    HtmlDoc::parse(s).unwrap().dom()
}

#[cfg(test)]
const HTML2: &str = r#"
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
fn rev_iter_toggle_text_and_comment() {
    let html = parse(HTML2);

    let el = html.root();
    let head = el.first_child().unwrap();
    let main = head.next_sibling().unwrap();
    let section = main.first_child().unwrap();
    let h4 = section.last_child_all().unwrap().prev_sibling().unwrap();
    assert_eq!(h4.tag(), HtmlTag::h4);

    let iter = RevElementIter::reverse(&h4).include_start();
    assert_eq!(
        iter.string(),
        "h4 30, a 27, p 24, h2 20, span 18, div 16, article 14, p 11, section 9, main 7, h1 4, header 2"
    );

    let iter = RevElementIter::reverse(&h4)
        .include_start()
        .set_include_text();
    assert_eq!(
        iter.string(),
        "h4 30, text 29, a 27, text 26, text 25, p 24, text 23, text 21, h2 20, text 19, span 18, text 17, div 16, text 15, article 14, text 13, p 11, text 10, section 9, text 8, main 7, text 6, text 5, h1 4, text 3, header 2, text 1"
    );

    let iter = RevElementIter::reverse(&h4)
        .include_start()
        .set_include_comment();
    assert_eq!(
        iter.string(),
        "h4 30, comment 28, a 27, p 24, comment 22, h2 20, span 18, div 16, article 14, comment 12, p 11, section 9, main 7, h1 4, header 2"
    );

    let iter = RevElementIter::reverse(&h4)
        .include_start()
        .set_include_text()
        .set_include_comment();
    assert_eq!(
        iter.string(),
        "h4 30, text 29, comment 28, a 27, text 26, text 25, p 24, text 23, comment 22, text 21, h2 20, text 19, span 18, text 17, div 16, text 15, article 14, text 13, comment 12, p 11, text 10, section 9, text 8, main 7, text 6, text 5, h1 4, text 3, header 2, text 1"
    );
}
