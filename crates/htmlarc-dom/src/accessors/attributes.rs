use std::cell::RefMut;

use crate::prelude::*;
use crate::stores::{Attribute, ListIndex, attr_list};

pub struct AttributesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) index: Option<ListIndex>,
}

impl AttributesMut<'_> {
    pub fn remove<F: Fn(Attribute) -> bool>(&mut self, f: F) -> usize {
        let Some(index) = self.index else {
            return 0;
        };
        attr_list::remove(&mut self.lock.attrs, index, f)
    }

    pub fn append(&mut self, attr: Attribute<'_>) {
        if let Some(index) = self.index {
            self.lock.attrs.list_mut_at(index).insert(&attr)
        } else {
            let index = self.lock.attrs.add_list(&attr);
            self.index = Some(index)
        }
    }
}

pub struct Attributes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    pub(crate) index: Option<ListIndex>,
}

impl<'dom, Dom: DomRef> Attributes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: u16) -> Self {
        let index = dom.dom_view().nodes.attr_list_index(node_index);
        Self { dom, index }
    }

    pub fn find_tag(&mut self, tag: HtmlAttr) -> Option<&str> {
        self.find(|attr| attr.tag == tag).map(|attr| attr.val)
    }
}

impl<'dom, Dom: DomRef> Iterator for Attributes<'dom, Dom> {
    type Item = Attribute<'dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.dom.dom_view().attrs.next_in_list(&mut self.index)
    }
}

#[test]
fn test_attr_iter() {
    const HTML: &str = r###"<body style="color: blue"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom();
    let mut iter = dom.root().forwards();
    let el = iter.next().unwrap(); // body
    let mut attrs = el.attributes();

    let v = attrs.find(|a| a.tag == HtmlAttr::style).map(|a| a.val);
    assert_eq!(v, Some("color: blue"));
}

#[test]
fn test_attr_mut() {
    const HTML: &str = r###"<body style="color: blue"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom_ref_cell();
    let body = dom.root();
    let body = body.first_child().unwrap(); // body

    body.attributes_mut()
        .remove(|attr| attr.tag == HtmlAttr::style);

    // should have no extra space after emptying the attribute list
    insta::assert_snapshot!(dom.to_html(HtmlFormat::Raw), @"<body></body>");
}
