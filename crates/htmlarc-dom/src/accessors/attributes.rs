use std::cell::RefMut;

use crate::prelude::*;
use crate::stores::{AttrName, Attribute, NAME_EXT_BASE, RunIndex};

/// Resolve an attribute name to its `NameSym`, interning an extended name into the document
/// symbol table (ADR 0002 §2). Std names are their `HtmlAttr` repr; extended names are
/// `(sym + NAME_EXT_BASE)`.
fn name_sym_or_insert(dom: &mut DomInner, name: AttrName<'_>) -> u16 {
    match name {
        AttrName::Std(attr) => attr as u16,
        AttrName::Ext(s) => dom.symbols.get_or_insert(s).as_u16() + NAME_EXT_BASE,
    }
}

pub struct AttributesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) node: NodeIndex,
    pub(crate) index: Option<RunIndex>,
}

impl AttributesMut<'_> {
    pub fn remove<F: Fn(Attribute) -> bool>(&mut self, f: F) -> usize {
        let Some(start) = self.index else {
            return 0;
        };
        let dom = &mut *self.lock;
        // Collect the doomed entry ids first (the predicate derefs each entry to an
        // `Attribute`, which needs both the attr value table and the symbol table). The
        // entries themselves stay until repackage compacts them.
        let doomed: Vec<u16> = {
            let attrs = dom.attrs.view();
            let symbols = dom.symbols.view();
            dom.attrs
                .lists
                .run_at(start)
                .filter(|&id| f(attrs.attribute_at(id, symbols)))
                .collect()
        };
        let (removed, emptied) = dom.attrs.lists.remove(start, &doomed);
        if emptied {
            // An empty run has no representation: drop the node's pointer so the attribute
            // disappears immediately (the slots are garbage until repackage).
            dom.nodes.set_attr_list_index(self.node, None);
            self.index = None;
        }
        removed
    }

    pub fn append(&mut self, attr: Attribute<'_>) {
        let dom = &mut *self.lock;
        let name_sym = name_sym_or_insert(dom, attr.name);
        if let Some(start) = self.index {
            let new_start = dom.attrs.append(start, name_sym, attr.val);
            if new_start != start {
                // The run was relocated to the arena end: re-point the node at it.
                dom.nodes
                    .set_attr_list_index(self.node, Some(new_start.as_u16()));
                self.index = Some(new_start);
            }
        } else {
            // The node had no attribute list yet: create one *and* point the node at it,
            // otherwise the new run is orphaned and the attribute is silently lost.
            let start = dom.attrs.new_run(name_sym, attr.val);
            dom.nodes
                .set_attr_list_index(self.node, Some(start.as_u16()));
            self.index = Some(start)
        }
    }
}

pub struct Attributes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    /// Cursor into the attribute-run arena (the next entry's offset), `None` once exhausted.
    pub(crate) index: Option<u16>,
}

impl<'dom, Dom: DomRef> Attributes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: NodeIndex) -> Self {
        let index = dom
            .dom_view()
            .nodes
            .attr_list_index(node_index)
            .map(|start| start.as_u16());
        Self { dom, index }
    }

    pub fn find_tag(&mut self, tag: HtmlAttr) -> Option<&str> {
        self.find(|attr| attr.name == AttrName::Std(tag))
            .map(|attr| attr.val)
    }
}

impl<'dom, Dom: DomRef> Iterator for Attributes<'dom, Dom> {
    type Item = Attribute<'dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.dom.dom_view().next_attr_in_run(&mut self.index)
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

    let v = attrs
        .find(|a| a.name == AttrName::Std(HtmlAttr::style))
        .map(|a| a.val);
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
        .remove(|attr| attr.name == AttrName::Std(HtmlAttr::style));

    // should have no extra space after emptying the attribute list
    insta::assert_snapshot!(dom.to_html(HtmlFormat::Raw), @"<body></body>");
}

#[test]
fn test_data_attr_iter() {
    // `data-*` and unknown names are ordinary extended attributes now.
    const HTML: &str = r###"<body data-hi="ho" wonky="yes"></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap();
    let dom = html.dom();
    let root = dom.root();
    let mut iter = root.forwards();
    let el = iter.next().unwrap(); // body
    let mut attrs = el.attributes();

    let v = attrs
        .find(|a| a.name == AttrName::Ext("data-hi"))
        .map(|a| a.val);
    assert_eq!(v, Some("ho"));
}
