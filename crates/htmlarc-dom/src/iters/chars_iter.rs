use crate::dom::{DomRead, DomRefCell, NodeIndex};
use crate::html::{HtmlElement, HtmlTag};

use super::DomIterator;
use super::node_chars::NodeChars;

pub struct CharsIter<'dom, Dom, Iter>
where
    Dom: DomRead,
    Iter: Iterator<Item = HtmlElement<'dom, Dom>> + DomIterator<'dom, Dom>,
    Self: 'dom,
{
    iter: Iter,
    chars: NodeChars,
    include_comments: bool,
    current: Option<NodeIndex>,
}

impl<'dom, Dom, Iter> CharsIter<'dom, Dom, Iter>
where
    Dom: DomRead,
    Iter: Iterator<Item = HtmlElement<'dom, Dom>> + DomIterator<'dom, Dom>,
    Self: 'dom,
{
    pub(crate) fn forward(iter: Iter) -> Self {
        Self {
            iter,
            chars: NodeChars::default(),
            include_comments: false,
            current: None,
        }
    }

    pub(crate) fn reverse(iter: Iter) -> Self {
        Self {
            iter,
            chars: NodeChars::rev(),
            include_comments: false,
            current: None,
        }
    }

    pub fn include_comments(mut self) -> Self {
        self.include_comments = true;
        self
    }

    pub fn current_char(&self) -> Option<char> {
        self.chars.current_char()
    }

    pub fn current_char_index(&self) -> Option<usize> {
        self.chars.current_index()
    }

    fn is_text(&self, tag: HtmlTag) -> bool {
        match tag {
            HtmlTag::sys_text => true,
            HtmlTag::sys_comment => self.include_comments,
            _ => false,
        }
    }

    fn next_element(&mut self) -> Option<HtmlElement<'dom, Dom>> {
        let Some(mut element) = self.iter.next() else {
            self.chars.reset(None);
            self.current = None;
            return None;
        };

        // move until we find an element that is allowed
        while !self.is_text(element.tag()) {
            let Some(new_element) = self.iter.next() else {
                self.chars.reset(None);
                self.current = None;
                return None;
            };
            element = new_element;
        }

        self.current = Some(element.index);
        Some(element)
    }

    fn next_element_char(&mut self) -> Option<char> {
        let element = self.next_element()?;
        self.chars.reset(element.text());
        self.chars.next()
    }

    #[cfg(test)]
    fn string(self) -> String {
        self.into_iter()
            .map(|c| c.to_string())
            .collect::<Vec<String>>()
            .join("")
    }
}

impl<'dom, Dom, Iter> Iterator for CharsIter<'dom, Dom, Iter>
where
    Dom: DomRead,
    Iter: Iterator<Item = HtmlElement<'dom, Dom>> + DomIterator<'dom, Dom>,
    Self: 'dom,
{
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(c) = self.chars.next() {
            // get the next character
            return Some(c);
        }

        // or try to get the char from the next element
        self.next_element_char()
    }
}

impl<'dom, Iter> CharsIter<'dom, DomRefCell, Iter>
where
    Iter: Iterator<Item = HtmlElement<'dom, DomRefCell>> + DomIterator<'dom, DomRefCell>,
    Self: 'dom,
{
    fn apply_changes(&self) {
        if let Some(index) = self.current {
            self.iter
                .dom()
                .with_mut(|dom| dom.replace_text(index, &self.chars.string()))
        }
    }
    pub fn remove_current(&mut self) {
        self.chars.remove_current();
        self.apply_changes();
    }

    pub fn replace_current(&mut self, c: char) {
        self.chars.replace_current(c);
        self.apply_changes();
    }

    pub fn insert_before_current(&mut self, c: char) {
        self.chars.insert(c, true);
        self.apply_changes();
    }

    pub fn insert_after_current(&mut self, c: char) {
        self.chars.insert(c, false);
        self.apply_changes();
    }
}

#[cfg(test)]
// head
// body
//   text: abc
//   div
//     b
//       text: d
//   article
//     h1
//       text: ef
//     p
//       text: ghij
//   section
const HTML: &str = "<head></head><body>abc<div><b>d</b></div><article><h1>ef</h1><p>ghij</p></article><section></section></body>";

#[cfg(test)]
use crate::prelude::*;

#[test]
fn forward_char_iter_plain() {
    let html = HtmlDoc::parse(HTML).unwrap().dom();
    assert_eq!(
        html.root()
            .forwards()
            .set_include_text()
            .map(|el| el.tag().to_string())
            .collect::<Vec<_>>()
            .join(", "),
        "head, body, text, div, b, text, article, h1, text, p, text, section"
    );
    let chars = html.root().forwards().text_chars();

    let char_string = chars.string();
    assert_eq!(char_string, "abcdefghij");
}

#[test]
fn forward_char_iter_replace() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let element = html.root();

    let mut chars = element.forwards().text_chars();
    chars.next(); // a
    chars.replace_current('z');
    chars.next(); // b
    chars.replace_current('y');
    chars.next(); // c
    chars.replace_current('x');
    chars.next(); // d
    chars.replace_current('w');
    chars.next(); // e
    chars.replace_current('v');
    chars.next(); // f
    chars.replace_current('u');
    chars.next(); // g
    chars.replace_current('t');
    chars.next(); // h
    chars.replace_current('s');
    chars.next(); // i
    chars.replace_current('r');
    chars.next(); // j
    chars.replace_current('q');

    let html = element.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>zyx<div><b>w</b></div><article><h1>vu</h1><p>tsrq</p></article><section></section></body>"
    );
}

#[test]
fn forward_char_iter_remove() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let element = html.root();

    let mut chars = element.forwards().text_chars();
    chars.next(); // a
    chars.remove_current();

    chars.next(); // b
    chars.remove_current();

    chars.next(); // c

    chars.next(); // d
    chars.remove_current();

    chars.next(); // e
    chars.remove_current();

    chars.next(); // f

    chars.next(); // g

    chars.next(); // h
    chars.remove_current();

    chars.next(); // i

    chars.next(); // j
    chars.remove_current();

    let html = element.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>c<div><b></b></div><article><h1>f</h1><p>gi</p></article><section></section></body>"
    );
}

#[test]
fn forward_char_iter_insert() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let element = html.root();

    let mut chars = element.forwards().text_chars();
    chars.next(); // a
    chars.insert_before_current('z');

    chars.next(); // b
    chars.insert_before_current('y');

    chars.next(); // c
    chars.insert_after_current('x');

    chars.next(); // x
    chars.insert_after_current('w');

    chars.next(); // w

    chars.next(); // d
    chars.insert_after_current('v');

    chars.next(); // v
    chars.insert_before_current('u');

    let html = element.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>zaybcxw<div><b>duv</b></div><article><h1>ef</h1><p>ghij</p></article><section></section></body>"
    );
}

#[test]
fn forward_char_iter_all_changes() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let element = html.root();

    let mut chars = element.forwards().text_chars();
    chars.next(); // a
    chars.remove_current();

    chars.next(); // b
    chars.insert_before_current('z');

    chars.next(); // c
    chars.replace_current('y');

    chars.next(); // d
    chars.insert_after_current('x');

    chars.next(); // x
    chars.replace_current('w');

    chars.next(); // e
    chars.insert_after_current('v');

    chars.next(); // v
    chars.insert_after_current('u');

    chars.next(); // u
    chars.remove_current();

    let html = element.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>zby<div><b>dw</b></div><article><h1>evf</h1><p>ghij</p></article><section></section></body>"
    );
}

// REVERSE

#[test]
fn reverse_char_iter_plain() {
    let html = HtmlDoc::parse(HTML).unwrap().dom();

    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();

    let chars = el.reverse().text_chars();

    let char_string = chars.string();
    assert_eq!(char_string, "jihgfedcba");
}

#[test]
fn reverse_char_iter_replace() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();

    let mut chars = el.reverse().text_chars();
    chars.next(); // j
    chars.replace_current('z');
    chars.next(); // i
    chars.replace_current('y');
    chars.next(); // h
    chars.replace_current('x');
    chars.next(); // g
    chars.replace_current('w');
    chars.next(); // f
    chars.replace_current('v');
    chars.next(); // e
    chars.replace_current('u');
    chars.next(); // d
    chars.replace_current('t');
    chars.next(); // c
    chars.replace_current('s');
    chars.next(); // b
    chars.replace_current('r');
    chars.next(); // a
    chars.replace_current('q');

    let html = el.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>qrs<div><b>t</b></div><article><h1>uv</h1><p>wxyz</p></article><section></section></body>"
    );
}

#[test]
fn reverse_char_iter_remove() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();

    let mut chars = el.reverse().text_chars();
    chars.next(); // j
    chars.remove_current();

    chars.next(); // i
    chars.remove_current();

    chars.next(); // h

    chars.next(); // g
    chars.remove_current();

    chars.next(); // f
    chars.remove_current();

    chars.next(); // e

    chars.next(); // d
    chars.remove_current();

    chars.next(); // c
    chars.remove_current();

    chars.next(); // b

    chars.next(); // a
    chars.remove_current();

    let html = el.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>b<div><b></b></div><article><h1>e</h1><p>h</p></article><section></section></body>"
    );
}

#[test]
fn reverse_char_iter_insert() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();

    let mut chars = el.reverse().text_chars();
    chars.next(); // j
    chars.insert_before_current('z');

    chars.next(); // z
    chars.insert_before_current('y');

    chars.next(); // y

    chars.next(); // i
    chars.insert_after_current('x');

    chars.next(); // h

    chars.next(); // g

    chars.next(); // f

    chars.next(); // e
    chars.insert_before_current('w');

    chars.next(); // w
    chars.insert_after_current('v');

    chars.next(); // d
    chars.insert_after_current('u');

    chars.next(); // c
    chars.insert_before_current('t');

    let html = el.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>abtc<div><b>du</b></div><article><h1>wvef</h1><p>ghixyzj</p></article><section></section></body>"
    );
}

#[test]
fn reverse_char_iter_all_changes() {
    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.last_child_all().unwrap();
    let el = el.last_child_all().unwrap();

    let mut chars = el.reverse().text_chars();
    chars.next(); // j
    chars.remove_current();

    chars.next(); // i
    chars.insert_before_current('z');

    chars.next(); // z
    chars.replace_current('y');

    chars.next(); // h
    chars.insert_after_current('x');

    chars.next(); // g
    chars.replace_current('w');

    chars.next(); // f
    chars.insert_before_current('v');

    chars.next(); // v
    chars.remove_current();

    let html = el.html_string(HtmlFormat::Raw);
    assert_eq!(
        html,
        "<head></head><body>abc<div><b>d</b></div><article><h1>ef</h1><p>whxyi</p></article><section></section></body>"
    );
}
