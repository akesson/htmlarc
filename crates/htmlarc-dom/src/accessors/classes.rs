use std::cell::RefMut;

use crate::{
    prelude::*,
    stores::{Class, ListIndex, ListRemovalResult, Sym},
};

pub struct ClassesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) node: NodeIndex,
    pub(crate) index: Option<ListIndex>,
}

impl ClassesMut<'_> {
    pub fn remove<F: Fn(&str) -> bool>(&mut self, f: F) -> usize {
        let Some(index) = self.index else {
            return 0;
        };
        let dom = &mut *self.lock;
        // Resolve the doomed class tokens to their list values (Syms) first; the strings
        // stay in the symbol table until repackage compacts them.
        let doomed: Vec<u16> = dom
            .class_lists
            .list_at(index)
            .filter(|&v| f(dom.symbols.get(Sym(v))))
            .collect();
        let mut count = 0;
        for v in doomed {
            if dom.class_lists.list_mut_at(index).remove(v) != ListRemovalResult::NotFound {
                count += 1;
            }
        }
        count
    }

    pub fn append(&mut self, class: &Class<'_>) {
        let dom = &mut *self.lock;
        let sym = dom.symbols.get_or_insert(class.0);
        if let Some(index) = self.index {
            dom.class_lists.list_mut_at(index).append(sym.as_u16());
        } else {
            // The node had no class list yet: create one *and* point the node at it,
            // otherwise the new list is orphaned and the class is silently lost.
            let index = dom.class_lists.new_list(sym.as_u16());
            dom.nodes
                .set_class_list_index(self.node, Some(index.as_u16()));
            self.index = Some(index)
        }
    }
}

pub struct Classes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    pub(crate) index: Option<ListIndex>,
}

impl<'dom, Dom: DomRef> Classes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: NodeIndex) -> Self {
        let index = dom.dom_view().nodes.class_list_index(node_index);
        Self { dom, index }
    }
}

impl<'dom, Dom: DomRef> Iterator for Classes<'dom, Dom> {
    type Item = &'dom str;

    fn next(&mut self) -> Option<Self::Item> {
        self.dom
            .dom_view()
            .next_class_in_list(&mut self.index)
            .map(|c| c.0)
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
