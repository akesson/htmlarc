pub(crate) use super::nodes::Nodes;
use super::{DomRead, DomRef, DomView, NodeIndex};
use crate::debug;
use crate::fmt::HtmlFormat;
use crate::html::HtmlElement;
use crate::iters::{DomIterator, RelativeIter, Tag, TagIter};
use crate::stores::{
    Attribute, AttributeStore, ClassStore, DataAttributeStore, ListIndex, StringStack,
};
use crate::{fmt::Spaces, html::HtmlTag};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

#[derive(Default, Archive, Serialize, Deserialize, Hash, Clone)]
pub struct DomInner {
    pub(crate) nodes: Nodes,
    pub(crate) attrs: AttributeStore,
    pub(crate) dataattrs: DataAttributeStore,
    pub(crate) classes: ClassStore,
    pub(crate) strings: StringStack,
}

impl Debug for DomInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DomInner")
            .field("node count", &self.nodes.len())
            .finish_non_exhaustive()
    }
}

impl DomInner {
    /// A borrowed read-only view over this document. The query layer reads through
    /// [`DomView`] so it is agnostic to owned vs. archived storage.
    pub(crate) fn view(&self) -> DomView<'_> {
        DomView::new(
            self.nodes.view(),
            self.attrs.view(),
            self.dataattrs.view(),
            self.classes.view(),
            self.strings.view(),
        )
    }

    pub fn append_text_child(&mut self, tag: HtmlTag, index: NodeIndex, text: &str) -> NodeIndex {
        debug_assert!(matches!(tag, HtmlTag::sys_comment | HtmlTag::sys_text));
        self.add_string_child(index, tag, text)
    }

    pub fn add_classes(&mut self, index: NodeIndex, classes: &str) -> Option<ListIndex> {
        let list_index = self.classes.add_class_list(classes)?;
        self.nodes
            .set_class_list_index(index, Some(list_index.as_u16()));
        Some(list_index)
    }

    pub fn add_attribute(
        &mut self,
        index: NodeIndex,
        list_index: Option<ListIndex>,
        attr: &Attribute,
    ) -> Option<ListIndex> {
        if let Some(attr_index) = list_index {
            self.attrs.list_mut_at(attr_index).insert(attr);
            list_index
        } else {
            let attr_index = self.attrs.add_list(attr);
            self.nodes
                .set_attr_list_index(index, Some(attr_index.as_u16()));
            Some(attr_index)
        }
    }

    pub fn replace_text(&mut self, index: NodeIndex, string: &str) {
        let range = self.strings.push(string);
        self.nodes.set_text_range(index, range);
    }

    fn add_string_child(&mut self, index: NodeIndex, tag: HtmlTag, string: &str) -> NodeIndex {
        let range = self.strings.push(string);
        let index = self.nodes.add_as_last_child(index, tag);
        self.nodes.set_text_range(index, range);
        index
    }

    pub(crate) fn string_at(&self, index: NodeIndex) -> &str {
        self.view().string_at(index)
    }

    pub(crate) fn starts_with_space(&self, index: NodeIndex) -> bool {
        let el = HtmlElement::new(self, index);
        debug!(
            "start (rev) {index}: '{}' {}",
            el.forwards().text_chars().collect::<String>(),
            self.nodes.tag(index)
        );
        if let Some(first_char) = el.forwards().text_chars().next() {
            first_char == ' '
        } else {
            false
        }
    }

    pub(crate) fn ends_with_space(&self, index: NodeIndex) -> bool {
        let el = HtmlElement::new(self, index);

        if let Some(last_char) = el.reverse().text_chars().next() {
            last_char == ' '
        } else {
            false
        }
    }

    pub fn insert_space_node_if_needed(
        &mut self,
        prev_sibling: Option<NodeIndex>,
        next_sibling: Option<NodeIndex>,
    ) {
        if let (Some(prev), Some(next)) = (prev_sibling, next_sibling)
            && self.nodes.is_inline_element(prev)
                && self.nodes.is_inline_element(next)
                // we use 'next' here as a parameter because the reverse chars iterator will ignore the provided index's characters
                && !self.ends_with_space(next)
                && !self.starts_with_space(next)
        {
            let space = self.nodes.add_as_next_sibling(prev, HtmlTag::sys_text);
            self.replace_text(space, " ");
        }
    }

    /// Replaces the current node with another one from the tree,
    pub fn replace_with(&mut self, index: NodeIndex, new_index: NodeIndex) {
        let is_block = self.nodes.is_block_element(index);
        let is_substitute_inline = self.nodes.is_inline_element(new_index);
        let prev_sibling = self.nodes.prev_sibling_index(index);
        let next_sibling = self.nodes.next_sibling_index(index);
        let Some(substitute_parent) = self.nodes.parent_index(new_index) else {
            panic!(
                "Substitute element is the root or is not in the tree and cannot be used as a replacement"
            );
        };

        self.nodes.replace_with(index, new_index);

        // prune the replacement node's parent
        let mut cursor = index;
        self.prune(&mut cursor, substitute_parent);

        if is_block && is_substitute_inline {
            // if the replaced node is a block element and the replacement node is an inline element

            // handle the space between the previous sibling and the new node
            self.insert_space_node_if_needed(prev_sibling, Some(new_index));

            // handle the space between the next sibling and the new node
            self.insert_space_node_if_needed(Some(new_index), next_sibling);
        }
    }

    /// Unwraps the current node, moving its children to its parent
    /// The cursor is moved to the next sibling if it exists, otherwise to the previous sibling
    pub fn unwrap_element(&mut self, index: NodeIndex) -> Option<NodeIndex> {
        let Some(parent) = self.nodes.parent_index(index) else {
            panic!("Element is the root or is not in the tree and cannot be unwrapped");
        };
        let is_block = self.nodes.is_block_element(index);
        let first_child = self.nodes.first_child_index(index);
        let last_child = self.nodes.last_child_index(index);
        let prev_sibling = self.nodes.prev_sibling_index(index);
        let next_sibling = self.nodes.next_sibling_index(index);

        let mut summaries = Vec::new();

        // collect every summary child of the unwrapped node
        {
            let el = HtmlElement::new(self, index);

            for child in RelativeIter::children(&el) {
                if child.tag() == HtmlTag::summary {
                    summaries.push(child.index());
                }
            }
        }

        // remove the summary elements
        for summary in summaries {
            self.nodes.remove(summary);
        }

        if let Some(new_index) = self.nodes.unwrap_node(index) {
            if is_block {
                // if the unwrapped node is a block element

                // handle the space between the previous sibling and the first child
                self.insert_space_node_if_needed(prev_sibling, first_child);

                // handle the space between the last child and the next sibling
                self.insert_space_node_if_needed(last_child, next_sibling);
            }

            Some(new_index)
        } else {
            // prune the parent because the node was removed, not unwrapped
            // NOTE: the pruning  will add spaces if necessary, so no need to handle them like in the block condition above
            let mut cursor = index;
            let new_index = self.prune(&mut cursor, parent);

            // and return the new element that wasn't pruned
            Some(new_index)
        }
    }

    pub fn prune(&mut self, cursor: &mut NodeIndex, index: NodeIndex) -> NodeIndex {
        let Some(parent) = self.nodes.parent_index(index) else {
            return index;
        };
        let tag = self.nodes.tag(index);
        let prev_sibling = self.nodes.prev_sibling_index(index);
        let next_sibling = self.nodes.next_sibling_index(index);
        let is_block = self.nodes.is_block_element(index);
        let is_childless = self.nodes.first_child_index(index).is_none();

        if tag == HtmlTag::body {
            let body = HtmlElement::new(self, index);
            let mut chars = body.descendants().text_chars();

            if chars.all(|c| c.is_whitespace()) || is_childless {
                // prune body element containing only whitespace or has no children
                if self.nodes.remove(index).is_some() {
                    // reposition the cursor index
                    *cursor = reposition_cursor(prev_sibling, next_sibling, parent);

                    // then consider pruning the parent
                    return self.prune(cursor, parent);
                }
            }
        } else if is_block {
            let el = HtmlElement::new(self, index);
            let mut chars = el.descendants().text_chars();

            if chars.all(|c| c.is_whitespace()) || is_childless {
                // prune the element
                if self.nodes.remove(index).is_some() {
                    // consider adding a space because we are removing a block element
                    self.insert_space_node_if_needed(prev_sibling, next_sibling);

                    // reposition the cursor index
                    *cursor = reposition_cursor(prev_sibling, next_sibling, parent);

                    // then consider pruning the parent
                    return self.prune(cursor, parent);
                }
            }
        } else if tag != HtmlTag::br && tag != HtmlTag::hr && is_childless {
            // prune empty element
            if self.nodes.remove(index).is_some() {
                // reposition the cursor index
                *cursor = reposition_cursor(prev_sibling, next_sibling, parent);

                // then consider pruning the parent
                return self.prune(cursor, parent);
            }
        }

        index
    }

    pub fn remove(&mut self, index: NodeIndex) -> Option<NodeIndex> {
        let Some(parent) = self.nodes.parent_index(index) else {
            panic!("Element is the root or is not in the tree and cannot be removed");
        };

        if let Some(new_index) = self.nodes.remove(index) {
            let prev_sibling = self.nodes.prev_sibling_index(index);
            let next_sibling = self.nodes.next_sibling_index(index);
            let is_block = self.nodes.is_block_element(index);

            if prev_sibling.is_some() || next_sibling.is_some() {
                // add a space if the removed node is a block element surrounded by inline elements with no preceding or leading space
                if is_block {
                    self.insert_space_node_if_needed(prev_sibling, next_sibling);
                }
            }

            // the pruning will replace block elements with a space and remove elements containing only whitespace
            // so a space will always be added if needed
            // the pruning will also reposition the cursor index if necessary
            let mut cursor = new_index;
            self.prune(&mut cursor, parent);
            Some(cursor)
        } else {
            None
        }
    }

    pub fn remove_formatting(&mut self) {
        let mut iter = TagIter::new(self);
        while let Some(elem) = iter.next(self) {
            if let Tag::Open(index) = elem {
                let tag = self.nodes.tag(index);

                if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
                    let mut string = { self.string_at(index).to_owned() };

                    let spaces = Spaces::count(&string);

                    if spaces.is_formatting() {
                        self.nodes.remove(index);
                    } else if spaces.nl > 0 || spaces.tab > 0 {
                        if spaces.tab > 0 {
                            string = string.replace('\t', "");
                        }
                        if spaces.nl > 0 {
                            string = string.trim_matches('\n').to_string();
                            string = string.replace('\n', " ");
                        }
                        self.replace_text(index, &string);
                    }
                }
            }
        }
    }

    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        fmt.to_html(self.view(), NodeIndex::ROOT)
    }

    /// Down-pack the node blob to the most compact width for serialization (u16 for
    /// small documents, u24 otherwise). Call this before archiving; the in-memory
    /// editable form is otherwise always u24.
    pub fn into_optimal_width(mut self) -> Self {
        let nodes = std::mem::take(&mut self.nodes).into_optimal_width();
        self.nodes = nodes;
        self
    }
}

impl ArchivedDomInner {
    /// A zero-copy [`DomView`] over the rkyv-archived document — every sub-view
    /// borrows directly from the (mmap'd) archived bytes.
    pub(crate) fn view(&self) -> DomView<'_> {
        DomView::new(
            self.nodes.view(),
            self.attrs.view(),
            self.dataattrs.view(),
            self.classes.view(),
            self.strings.view(),
        )
    }
}

impl Debug for ArchivedDomInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivedDomInner")
            .field("node count", &self.nodes.view().len())
            .finish_non_exhaustive()
    }
}

impl DomRead for ArchivedDomInner {
    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.view())
    }

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::new(self, NodeIndex::ROOT)
    }

    fn repackage(&self) -> DomInner {
        rkyv::deserialize::<DomInner, rkyv::rancor::Error>(self)
            .expect("archived DomInner must deserialize")
    }
}

impl DomRef for ArchivedDomInner {
    fn dom_view(&self) -> DomView<'_> {
        self.view()
    }
}

fn reposition_cursor(
    prev_sibling: Option<NodeIndex>,
    next_sibling: Option<NodeIndex>,
    parent: NodeIndex,
) -> NodeIndex {
    if let Some(prev_sibling) = prev_sibling {
        prev_sibling
    } else if let Some(next_sibling) = next_sibling {
        next_sibling
    } else {
        parent
    }
}

#[cfg(test)]
use crate::prelude::*;

#[test]
fn test_adding_space() {
    let html_str = "<strong><b>a</b></strong><em><i>b</i></em>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let em = el.index();

    html.with_mut(|dom| dom.insert_space_node_if_needed(Some(strong), Some(em)));

    assert_eq!(
        html.to_html(HtmlFormat::Pretty).trim(),
        "<strong><b>a</b></strong> <em><i>b</i></em>",
        "Should add space between inline elements"
    );

    let html_str = "<strong><b>a </b></strong><em>b</em>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let em = el.index();
    html.with_mut(|dom| dom.insert_space_node_if_needed(Some(strong), Some(em)));
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<strong><b>a </b></strong><em>b</em>",
        "Should not add a space if the preceding element is ending with a space"
    );

    let html_str = "<strong><b>a</b></strong><em> b</em>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let strong = el.index();
    let el = el.next_sibling().unwrap();
    let em = el.index();
    html.with_mut(|dom| dom.insert_space_node_if_needed(Some(strong), Some(em)));
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<strong><b>a</b></strong><em> b</em>",
        "Should not add a space if the following element is starting with a space"
    );

    let html_str = "<div><b>a</b></div><em>b</em>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let div = el.index();
    let el = el.next_sibling().unwrap();
    let em = el.index();
    html.with_mut(|dom| dom.insert_space_node_if_needed(Some(div), Some(em)));
    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<div><b>a</b></div><em>b</em>",
        "Should not add space between block and inline elements"
    );
}

#[test]
fn prune_body_element() {
    let html_str =
        "<head><meta></head><section><div><body><p> </p>   <div> </div></body></div></section>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    let el = el.next_sibling().unwrap();
    html.with_mut(|dom| {
        dom.prune(&mut NodeIndex::new(0), el.index());
    });

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<head><meta></head>",
        "Body element containing only whitespaces should be pruned along with all its ancestors that are empty"
    );
}

#[test]
fn prune_empty_block() {
    let html_str = "<div><p></p><p> </p><p> </p></div>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap();
    html.with_mut(|dom| dom.prune(&mut NodeIndex::new(0), el.index()));

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "",
        "Empty block element should be pruned"
    );

    let html_str = "<body><i>italic</i><section><div><p></p></div></section><b>bold</b></body>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // body
    let el = el.first_child().unwrap(); // i
    let el = el.next_sibling().unwrap(); // section
    let el = el.first_child().unwrap(); // div
    let el = el.first_child().unwrap(); // p
    html.with_mut(|dom| dom.prune(&mut NodeIndex::new(0), el.index()));

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "<body><i>italic</i> <b>bold</b></body>",
        "After pruning an empty block element, its ancestors should be pruned or replaced with a space if it's between inline elements"
    );
}

#[test]
fn prune_empty_elements() {
    let html_str = "<a><i><b><sub></sub></b></i></a>";
    let html = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();
    let el = html.root();
    let el = el.first_child().unwrap(); // a
    let el = el.first_child().unwrap(); // i
    let el = el.first_child().unwrap(); // b
    let el = el.first_child().unwrap(); // sub
    html.with_mut(|dom| dom.prune(&mut NodeIndex::new(0), el.index()));

    assert_eq!(
        html.to_html(HtmlFormat::Raw),
        "",
        "Empty elements should be pruned along with all their ancestors that are empty"
    );
}

// ============================================================================
// [SPIKE] Zero-copy mmap feasibility — Part 1 (htmlarc-dom internals)
//
// Proves the load-bearing claim of the plan: the rkyv-*archived* form of a
// `DomInner` exposes the very same byte slices the owned form uses, so a future
// `DomView`/`NodesView` over `&[u8]`/`&str` works identically for both — no
// per-document deserialization required to read.
// ============================================================================
#[test]
fn spike_zero_copy_archived_dom() {
    use rkyv::rancor::Error;

    let html = r#"<body><h1 class="title" id="t">Hello</h1><p>world &amp; more</p></body>"#;
    let dom: DomInner = HtmlDoc::parse(html).unwrap().inner();

    // Serialize exactly as the archive does (`rkyv::to_bytes`).
    let bytes = rkyv::to_bytes::<Error>(&dom).unwrap();

    // SAFE, validated, zero-copy access. bytecheck is already compiled in
    // (workspace does not set default-features=false), so this needs NO new
    // flags/derives and returns Err — never UB — on a malformed buffer.
    let archived: &ArchivedDomInner =
        rkyv::access::<ArchivedDomInner, Error>(&bytes[..]).expect("safe access must succeed");

    // (1) THE proof: the node-topology blob is byte-identical owned vs archived.
    //     Every `Nodes` read accessor is `from_le_bytes` over this slice, so if the
    //     bytes match, a view over `&[u8]` decodes identically for both backings.
    assert_eq!(
        dom.nodes.view().as_bytes(),
        archived.nodes.view().as_bytes(),
        "archived node blob must be byte-identical to owned"
    );

    // (2) Same for the text/comment string pool.
    assert_eq!(
        dom.strings.view().as_bytes(),
        archived.strings.view().as_bytes()
    );

    // (3) Decode straight off the archived &[u8] with the real layout constants and
    //     confirm it agrees with the owned accessor — i.e. a view would Just Work.
    // owned/un-downpacked documents are built at u24 (22-byte records)
    const NODE_SIZE: usize = 22;
    let ab = archived.nodes.view().as_bytes();
    assert!(
        ab.len().is_multiple_of(NODE_SIZE),
        "blob is a whole number of nodes"
    );
    assert_eq!(
        ab[0],
        dom.nodes.view().as_bytes()[0],
        "root tag byte matches"
    );
    assert_eq!(
        ab[NODE_SIZE],
        dom.nodes.tag(NodeIndex::new(1)) as u8,
        "node 1 (body) tag decodes identically off the archived slice"
    );

    // (4) Escape hatch for mutation: archived -> owned via rkyv::deserialize, and the
    //     round-trip renders identically (this is what rebuild()/repackage() back).
    let owned_again: DomInner = rkyv::deserialize::<DomInner, Error>(archived).unwrap();
    assert_eq!(
        dom.to_html(HtmlFormat::Raw),
        owned_again.to_html(HtmlFormat::Raw),
        "deserialized round-trip is identical"
    );
}

// A document larger than the old u16 ceiling stays u24 and survives a zero-copy
// round-trip, with node indices/links beyond `u16::MAX` intact — the headroom proof.
#[test]
fn large_document_uses_u24_and_exceeds_u16() {
    use rkyv::rancor::Error;

    let n: u32 = 70_000; // > u16::MAX
    assert!(n > u16::MAX as u32);

    let mut dom = DomInner::default();
    for _ in 0..n {
        dom.nodes.add_as_last_child(NodeIndex::ROOT, HtmlTag::div);
    }
    assert_eq!(dom.nodes.len(), (n + 1) as usize);

    // above the down-pack margin, so it stays u24
    let dom = dom.into_optimal_width();

    let bytes = rkyv::to_bytes::<Error>(&dom).unwrap();
    let archived = rkyv::access::<ArchivedDomInner, Error>(&bytes[..]).expect("zero-copy access");

    let view = archived.view();
    assert_eq!(view.nodes.len(), (n + 1) as usize);

    // a node AND a link value past u16::MAX round-trip correctly
    let last = NodeIndex::new(n);
    assert_eq!(view.nodes.last_child_index(NodeIndex::ROOT), Some(last));
    assert_eq!(view.nodes.parent_index(last), Some(NodeIndex::ROOT));
    assert_eq!(
        view.nodes.prev_sibling_index(last),
        Some(NodeIndex::new(n - 1))
    );
    assert_eq!(view.nodes.next_sibling_index(last), None);
}

#[test]
fn test_reposition_cursor() {
    let index = reposition_cursor(
        Some(NodeIndex::new(0)),
        Some(NodeIndex::new(1)),
        NodeIndex::new(2),
    );
    assert_eq!(
        index,
        NodeIndex::new(0),
        "Should prioritize moving the cursor to the previous sibling"
    );

    let index = reposition_cursor(None, Some(NodeIndex::new(1)), NodeIndex::new(2));
    assert_eq!(
        index,
        NodeIndex::new(1),
        "Should move the cursor to the next sibling if there is no previous sibling"
    );

    let index = reposition_cursor(None, None, NodeIndex::new(2));
    assert_eq!(
        index,
        NodeIndex::new(2),
        "Should move the cursor to the parent if there are no siblings"
    );
}
