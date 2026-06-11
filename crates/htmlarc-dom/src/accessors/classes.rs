use std::cell::RefMut;

use crate::{
    prelude::*,
    stores::{Class, RunIndex, Sym},
};

pub struct ClassesMut<'a> {
    pub(crate) lock: RefMut<'a, DomInner>,
    pub(crate) node: NodeIndex,
    pub(crate) index: Option<RunIndex>,
}

impl ClassesMut<'_> {
    pub fn remove<F: Fn(&str) -> bool>(&mut self, f: F) -> usize {
        let Some(start) = self.index else {
            return 0;
        };
        let dom = &mut *self.lock;
        // Resolve the doomed class tokens to their run values (Syms) first; the strings
        // stay in the symbol table until repackage compacts them.
        let doomed: Vec<u16> = dom
            .class_lists
            .run_at(start)
            .filter(|&v| f(dom.symbols.get(Sym(v))))
            .collect();
        let (removed, emptied) = dom.class_lists.remove(start, &doomed);
        if emptied {
            // An empty run has no representation: drop the node's pointer so the class
            // attribute disappears immediately (the slots are garbage until repackage).
            dom.nodes.set_class_list_index(self.node, None);
            self.index = None;
        }
        removed
    }

    pub fn append(&mut self, class: &Class<'_>) {
        let dom = &mut *self.lock;
        let sym = dom.symbols.get_or_insert(class.0);
        if let Some(start) = self.index {
            let new_start = dom.class_lists.append(start, sym.as_u16());
            if new_start != start {
                // The run was relocated to the arena end: re-point the node at it.
                dom.nodes
                    .set_class_list_index(self.node, Some(new_start.as_u16()));
                self.index = Some(new_start);
            }
        } else {
            // The node had no class list yet: create one *and* point the node at it,
            // otherwise the new run is orphaned and the class is silently lost.
            let start = dom.class_lists.new_run(sym.as_u16());
            dom.nodes
                .set_class_list_index(self.node, Some(start.as_u16()));
            self.index = Some(start)
        }
    }
}

pub struct Classes<'dom, Dom: DomRef> {
    pub(crate) dom: &'dom Dom,
    /// Cursor into the class-run arena (the next value's offset), `None` once exhausted.
    pub(crate) index: Option<u16>,
}

impl<'dom, Dom: DomRef> Classes<'dom, Dom> {
    pub fn new(dom: &'dom Dom, node_index: NodeIndex) -> Self {
        let index = dom
            .dom_view()
            .nodes
            .class_list_index(node_index)
            .map(|start| start.as_u16());
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
