use std::cell::RefMut;

use log::debug;

use crate::{
    prelude::*,
    stores::{DataAttribute, ListIndex, data_attr_list},
};

pub struct DataAttributesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) index: Option<ListIndex>,
}

impl DataAttributesMut<'_> {
    pub fn remove<F: Fn(DataAttribute) -> bool>(&mut self, f: F) -> usize {
        debug!("remove, index: {:?}", self.index);
        let Some(index) = self.index else {
            return 0;
        };

        debug!("index: {:?}", index);
        data_attr_list::remove(&mut self.lock.dataattrs, index, f)
    }

    pub fn append(&mut self, attr: &DataAttribute<'_>) {
        if let Some(index) = self.index {
            self.lock.dataattrs.list_mut_at(index).insert(attr)
        } else {
            let index = self.lock.dataattrs.add_list(attr);
            self.index = Some(index)
        }
    }
}

pub struct DataAttributes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    pub(crate) index: Option<ListIndex>,
}

impl<'dom, Dom: DomRef> DataAttributes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: u16) -> Self {
        let index = dom.as_ref().nodes.data_attr_list_index(node_index);
        Self { dom, index }
    }

    pub fn find_tag(&mut self, tag: &str) -> Option<&str> {
        self.find(|attr| attr.tag == tag).map(|attr| attr.val)
    }
}

impl<'dom, Dom: DomRef> Iterator for DataAttributes<'dom, Dom> {
    type Item = DataAttribute<'dom>;

    fn next(&mut self) -> Option<Self::Item> {
        data_attr_list::next(&self.dom.as_ref().dataattrs, &mut self.index)
    }
}

#[test]
fn test_data_attr_iter() {
    const HTML: &str = r###"<body data-hi="ho"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom();
    let root = dom.root();
    let mut iter = root.forwards();
    let el = iter.next().unwrap(); // body
    let mut d_attrs = el.data_attributes();

    let v = d_attrs.find(|a| a.tag == "hi").map(|a| a.val);
    assert_eq!(v, Some("ho"));
}

#[ignore = "doesn't yet handle cases when there are no list present"]
#[test]
fn test_attr_mut_none() {
    const HTML: &str = r###"<body style="color: blue"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom_ref_cell();
    let root = dom.root();
    let mut iter = root.forwards();
    let el = iter.next().unwrap(); // body
    let mut attr = el.data_attributes_mut();
    attr.remove(|attr| attr.tag == "hi");

    drop(attr);
    drop(iter);
    // should have no extra space after emptying the attribute list
    insta::assert_snapshot!(dom.to_html(HtmlFormat::Raw), @"<body></body>");
}

#[test]
fn test_attr_mut() {
    const HTML: &str = r###"<body data-hi="ho"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom_ref_cell();
    let root = dom.root();
    let mut iter = root.forwards();
    let el = iter.next().unwrap(); // body
    let mut attr = el.data_attributes_mut();
    attr.remove(|attr| {
        println!("{:?}", attr);
        attr.tag == "hi"
    });

    drop(attr);
    drop(iter);
    // should have no extra space after emptying the attribute list
    insta::assert_snapshot!(dom.to_html(HtmlFormat::Raw), @"<body></body>");
}
