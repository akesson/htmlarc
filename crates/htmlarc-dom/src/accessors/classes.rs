use std::cell::RefMut;

use crate::{
    prelude::*,
    stores::{Class, ListIndex, class_list},
};

pub struct ClassesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) index: Option<ListIndex>,
}

impl ClassesMut<'_> {
    pub fn remove<F: Fn(&str) -> bool>(&mut self, f: F) -> usize {
        let Some(index) = self.index else {
            return 0;
        };
        class_list::remove(&mut self.lock.classes, index, f)
    }

    pub fn append(&mut self, class: &Class<'_>) {
        if let Some(index) = self.index {
            self.lock.classes.list_mut_at(index).insert(class)
        } else {
            let index = self.lock.classes.add_list(class);
            self.index = Some(index)
        }
    }
}

pub struct Classes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    pub(crate) index: Option<ListIndex>,
}

impl<'dom, Dom: DomRef> Classes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: u16) -> Self {
        let index = dom.as_ref().nodes.class_list_index(node_index);
        Self { dom, index }
    }
}

impl<'dom, Dom: DomRef> Iterator for Classes<'dom, Dom> {
    type Item = &'dom str;

    fn next(&mut self) -> Option<Self::Item> {
        class_list::next(&self.dom.as_ref().classes, &mut self.index).map(|c| c.0)
    }
}

#[test]
fn test_classes_iter() {
    const HTML: &str = r###"<body class="mw-hi bla"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom();
    let mut iter = dom.root().forwards();
    let el = iter.next().unwrap(); // body
    let mut classes = el.classes();

    let v = classes.find(|c| c.starts_with("mw"));
    assert_eq!(v, Some("mw-hi"));
}

#[test]
fn test_attr_mut() {
    const HTML: &str = r###"<body class="mw-hi bla"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom_ref_cell();
    let root = dom.root();

    let mut iter = root.forwards();
    let el = iter.next().unwrap(); // body
    let mut attr = el.classes_mut();
    attr.remove(|class| class == "mw-hi");

    // there's a space after the start element name, which
    // only happens when a list has been removed and it will
    // go away on next repack.
    drop(iter);
    drop(attr);
    insta::assert_snapshot!(dom.to_html(HtmlFormat::Raw), @r###"<body class="bla"></body>"###);
}
