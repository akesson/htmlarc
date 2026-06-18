pub(crate) use super::nodes::Nodes;
use super::nodes::TopologyReport;
use super::{DomRead, DomRef, DomView, NodeIndex};
use crate::debug;
use crate::fmt::HtmlFormat;
use crate::html::HtmlElement;
use crate::iters::{DomIterator, RelativeIter, Tag, TagIter};
use crate::stores::{AttrStore, ExtTags, RunVec, StringSource, StringStack, SymbolTable};
use crate::{fmt::Spaces, html::HtmlTag};
use rkyv::{Archive, Deserialize, Serialize};
use std::fmt::Debug;
use std::hash::Hash;

#[derive(Default, Archive, Serialize, Deserialize, Hash, Clone)]
pub struct DomInner {
    pub(crate) nodes: Nodes,
    /// Every attribute of every node — standard, `data-*`, and unknown alike (ADR 0002 §3).
    pub(crate) attrs: AttrStore,
    /// Deduplicated identity strings: class tokens and extended attribute names. Class lists
    /// in `class_lists` and the attribute store's entry names index this table.
    pub(crate) symbols: SymbolTable,
    /// Per-node class lists as contiguous runs of `Sym` values; a node's class slot holds
    /// its run's start offset directly.
    pub(crate) class_lists: RunVec,
    /// Extended (custom/unknown) tag names: the per-document vocab + overflow side map that a
    /// node's `>= EXT_BASE` tag byte resolves through (ADR 0002 §4). Names are `Sym`s into
    /// `symbols`, shared with class tokens and extended attribute names.
    pub(crate) ext_tags: ExtTags,
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
            self.symbols.view(),
            self.class_lists.view(),
            self.ext_tags.view(),
            self.strings.view(),
        )
    }

    pub(crate) fn append_text_child(
        &mut self,
        tag: HtmlTag,
        index: NodeIndex,
        text: &str,
    ) -> NodeIndex {
        debug_assert!(matches!(tag, HtmlTag::sys_comment | HtmlTag::sys_text));
        self.add_string_child(index, tag, text)
    }

    /// Test helper: attach a class list to a node. Used only by node tests.
    #[cfg(test)]
    pub(crate) fn add_classes(&mut self, index: NodeIndex, classes: &str) {
        let mut names = classes.split_ascii_whitespace();
        let first = self.symbols.get_or_insert(names.next().unwrap_or(""));
        let mut start = self.class_lists.new_run(first.as_u16());
        for name in names {
            let sym = self.symbols.get_or_insert(name);
            start = self.class_lists.append(start, sym.as_u16());
        }
        self.nodes.set_class_list_index(index, Some(start.as_u16()));
    }

    pub(crate) fn replace_text(&mut self, index: NodeIndex, string: &str) {
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

    pub(crate) fn insert_space_node_if_needed(
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
    pub(crate) fn replace_with(&mut self, index: NodeIndex, new_index: NodeIndex) {
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
    pub(crate) fn unwrap_element(&mut self, index: NodeIndex) -> Option<NodeIndex> {
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

    pub(crate) fn prune(&mut self, cursor: &mut NodeIndex, index: NodeIndex) -> NodeIndex {
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

    pub(crate) fn remove(&mut self, index: NodeIndex) -> Option<NodeIndex> {
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

    /// Strip formatting-only whitespace text nodes and collapse indentation.
    ///
    /// A high-level whole-document mutation (no node indices), callable via
    /// [`DomRefCell::with_mut`](crate::dom::DomRefCell::with_mut).
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

    /// Take the document's text/comment pool, leaving it empty. Used when relocating strings
    /// into a per-bundle store: the bytes move to the bundle and the per-document blob is
    /// serialized string-less. The node text ranges stay valid against the relocated segment.
    pub fn take_string_pool(&mut self) -> Vec<u8> {
        self.strings.take_bytes()
    }

    /// Install a text/comment pool — the inverse of [`take_string_pool`](Self::take_string_pool),
    /// used when an archive that relocated its strings is loaded fully into memory and each
    /// document's segment is re-attached to its owned [`DomInner`].
    pub fn set_string_pool(&mut self, bytes: Vec<u8>) {
        self.strings = StringStack::from_bytes(bytes);
    }

    /// ADR 0002 topology-packing probe: tally the redundancy in the node-link slots so the
    /// delta/implicit-link packing ceiling can be measured on real corpora before committing
    /// to an encoding. Read-only; call on the serialized form (after [`into_optimal_width`])
    /// to measure the on-disk topology, or after [`rebuild`](Self::rebuild) for the
    /// document-order baseline.
    pub fn topology_report(&self) -> TopologyReport {
        self.nodes.view().topology_report()
    }
}

impl ArchivedDomInner {
    /// Assemble a zero-copy [`DomView`] over the rkyv-archived document. The topology/attribute
    /// sub-views borrow directly from the (mmap'd) archived bytes; the *text* pool is supplied
    /// separately as a [`StringSource`] because a relocated document's strings live in its bundle,
    /// not in its own blob. [`ArchivedDom`] pairs the two.
    pub(crate) fn view_with<'a>(&'a self, strings: StringSource<'a>) -> DomView<'a> {
        DomView::new(
            self.nodes.view(),
            self.attrs.view(),
            self.symbols.view(),
            self.class_lists.view(),
            self.ext_tags.view(),
            strings,
        )
    }

    /// Bind this archived document to an external text source (its bundle's per-document
    /// segment). The reader uses this for relocated archives.
    pub fn with_strings<'a>(&'a self, strings: StringSource<'a>) -> ArchivedDom<'a> {
        ArchivedDom {
            inner: self,
            strings,
        }
    }

    /// Bind using the document's *own* inline string pool — valid when the blob was serialized
    /// whole (a standalone `rkyv::to_bytes` round-trip, or a non-relocated archive). For a
    /// relocated blob the inline pool is empty, so the reader must use
    /// [`with_strings`](Self::with_strings) instead.
    pub fn bound(&self) -> ArchivedDom<'_> {
        let strings = self.strings.view();
        self.with_strings(strings)
    }
}

impl Debug for ArchivedDomInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivedDomInner")
            .field("node count", &self.nodes.view().len())
            .finish_non_exhaustive()
    }
}

/// An archived document bound to its text source — the readable, queryable form of a
/// memory-mapped document. It pairs a borrowed [`ArchivedDomInner`] (topology/attributes, read
/// zero-copy from the mmap) with the [`StringSource`] that resolves its text (the document's
/// segment of its bundle's string pool). It implements [`DomRead`]/[`DomRef`] so the query layer
/// reads it exactly like an owned [`DomInner`].
#[derive(Clone, Copy)]
pub struct ArchivedDom<'a> {
    inner: &'a ArchivedDomInner,
    strings: StringSource<'a>,
}

impl<'a> ArchivedDom<'a> {
    fn view(&self) -> DomView<'a> {
        self.inner.view_with(self.strings)
    }
}

impl Debug for ArchivedDom<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArchivedDom")
            .field("node count", &self.inner.nodes.view().len())
            .finish_non_exhaustive()
    }
}

impl DomRead for ArchivedDom<'_> {
    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.view())
    }

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::new(self, NodeIndex::ROOT)
    }

    fn repackage(&self) -> DomInner {
        // Topology deserializes straight from the blob; the (relocated) text is materialised
        // from the bound source into a fresh owned pool, leaving the document fully self-owned.
        let mut dom = rkyv::deserialize::<DomInner, rkyv::rancor::Error>(self.inner)
            .expect("archived DomInner must deserialize");
        dom.set_string_pool(self.strings.materialize());
        dom
    }
}

impl DomRef for ArchivedDom<'_> {
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
    let dom: DomInner = HtmlDoc::parse(html).unwrap().dom();

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
    // owned/un-downpacked documents are built at u24 (20-byte records after the data slot
    // was dropped in ADR 0002 PR 3: tag(1) + 5 link slots + 2 store slots, all 3-byte/u24).
    const NODE_SIZE: usize = 20;
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

    let view = archived.bound().view();
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

// Regression: removing *all* entries from a class/attribute list left the owning node
// pointing at a now-empty list head. `mark_list_used` skips empty heads, so on repackage
// the rebuilder either unwrapped a `None` (rebuilder.rs) or — when the emptied list sat at
// slot 0 — failed every other list's "no next" reindex ("Invalid reindex", see
// `ListInfo::reindexed`). Emptying a list must instead drop the node's pointer cleanly.
#[test]
fn repackage_after_emptying_a_class_list() {
    // `<html class>` is the first classed element, so its list occupies slot 0 — the exact
    // case that used to corrupt every other list when emptied.
    let html_str = "<html class='root'><head></head><body>\
        <p class='keep me'>x</p><span class='drop'>y</span></body></html>";
    let dom = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    dom.root()
        .first_child_tag(HtmlTag::html)
        .unwrap()
        .classes_mut()
        .remove(|_| true);

    let html = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        !html.contains("class=\"root\""),
        "emptied <html> class should be gone: {html}"
    );
    assert!(
        !html.contains("class=\"\""),
        "emptied list must not leave an empty class attr: {html}"
    );
    assert!(
        html.contains("class=\"keep me\""),
        "<p> classes must survive: {html}"
    );
    assert!(
        html.contains("class=\"drop\""),
        "<span> class must survive: {html}"
    );
}

// Regression: appending a class/attribute to an element that had *no* list yet created
// the list in the store but never pointed the node at it, so the value was silently lost
// (a fresh `<meta charset>` rendered as `<meta>`). The node pointer must be set too.
#[test]
fn append_to_element_without_existing_list() {
    let dom = HtmlDoc::parse("<html><head></head><body></body></html>")
        .unwrap()
        .dom_ref_cell();

    let head = dom.root().path([HtmlTag::html, HtmlTag::head]).unwrap();
    head.append_child(HtmlTag::meta)
        .attributes_mut()
        .append(Attribute {
            name: AttrName::Std(HtmlAttr::charset),
            val: "UTF-8",
        });

    dom.root()
        .path([HtmlTag::html, HtmlTag::body])
        .unwrap()
        .classes_mut()
        .append(&Class::from("added"));

    // Persisted immediately…
    let raw = dom.to_html(HtmlFormat::Raw);
    assert!(
        raw.contains("charset=\"UTF-8\""),
        "charset must persist: {raw}"
    );
    assert!(raw.contains("class=\"added\""), "class must persist: {raw}");

    // …and survives a repackage.
    let repacked = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        repacked.contains("charset=\"UTF-8\""),
        "charset must survive repackage: {repacked}"
    );
    assert!(
        repacked.contains("class=\"added\""),
        "class must survive repackage: {repacked}"
    );
}

#[test]
fn repackage_after_emptying_an_attr_list() {
    // Same arena/rebuild path, exercised through the attribute store.
    let html_str = "<html><head></head><body>\
        <a id='gone' href='/x'>link</a><p data-k='v'>keep</p></body></html>";
    let dom = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    let anchor = dom.root().select_css("a").unwrap().first().unwrap();
    anchor.attributes_mut().remove(|_| true);

    let html = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        !html.contains("id=\"gone\""),
        "emptied <a> attributes should be gone: {html}"
    );
    assert!(
        !html.contains("href=\"/x\""),
        "emptied <a> attributes should be gone: {html}"
    );
    assert!(
        html.contains("data-k"),
        "<p> data attribute must survive: {html}"
    );
}

// At U16 the node record is 15 bytes; a text node's `u32` start/end overlay needs exactly
// `1 + 3*2 + 8 = 15` bytes, and an element's class+attr store slots also end at 15 — the
// binding layout constraint after the data slot was dropped (ADR 0002 §3). A small document
// mixing text with attr/class-bearing elements must down-pack to U16 (15-byte stride) and
// survive a zero-copy round-trip with its text and attributes intact.
#[test]
fn u16_node_record_text_overlay_boundary() {
    use rkyv::rancor::Error;

    let html = r#"<p class="c" id="i">hello<b data-x="y">bold</b> tail</p>"#;
    let dom: DomInner = HtmlDoc::parse(html).unwrap().dom().into_optimal_width();
    let raw = dom.to_html(HtmlFormat::Raw);

    let bytes = rkyv::to_bytes::<Error>(&dom).unwrap();
    let archived = rkyv::access::<ArchivedDomInner, Error>(&bytes[..]).unwrap();
    assert!(
        archived.nodes.view().as_bytes().len().is_multiple_of(15),
        "small document must down-pack to the 15-byte U16 node stride"
    );

    let owned_again: DomInner = rkyv::deserialize::<DomInner, Error>(archived).unwrap();
    assert_eq!(
        owned_again.to_html(HtmlFormat::Raw),
        raw,
        "U16 zero-copy round-trip preserves text ranges and attributes"
    );
    assert!(raw.contains("data-x=\"y\"") && raw.contains("hello") && raw.contains(" tail"));
}

// Regression for the ADR 0002 §3 rebuild trap: extended attribute names live in the *same*
// per-document symbol table as class tokens, but the class rebuilder only marks class-used
// syms. Without the union pass in `DomInner::rebuild`, repackaging a document whose classes
// are compacted would drop every `data-*`/unknown name and leave its `NameSym` dangling.
#[test]
fn repackage_keeps_extended_attr_names_when_classes_compact() {
    // The <p class="drop"> is emptied, forcing the symbol table to compact; the <a> on a
    // different element carries extended names (`data-mw`, the unknown `wonky`) that must
    // survive that compaction with their values intact.
    let html_str = "<html class='root'><body>\
        <p class='drop keep'>x</p>\
        <a data-mw='interface' wonky='yes' href='/x'>link</a></body></html>";
    let dom = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    dom.root()
        .select_css("p")
        .unwrap()
        .first()
        .unwrap()
        .classes_mut()
        .remove(|_| true);

    let html = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        html.contains("data-mw=\"interface\""),
        "extended name+value must survive symbol compaction: {html}"
    );
    assert!(
        html.contains("wonky=\"yes\""),
        "unknown extended name+value must survive: {html}"
    );
    assert!(
        html.contains("href=\"/x\""),
        "std attr must survive: {html}"
    );
    assert!(
        html.contains("class=\"root\""),
        "the untouched <html> class must survive: {html}"
    );
    assert!(
        !html.contains("drop"),
        "the emptied <p> class is gone: {html}"
    );
}

// Regression for the ADR 0002 §4 rebuild trap: extended *tag* names live in the same
// per-document symbol table as class tokens and extended attribute names, but the class
// rebuilder only marks class-used syms. Without the tag-name union pass in `DomInner::rebuild`
// (the twin of the attr-name pass), repackaging a document whose classes are compacted would
// drop every custom element's name and leave its `Sym` dangling.
#[test]
fn repackage_keeps_extended_tag_names_when_classes_compact() {
    // The <p class="drop keep"> is emptied, forcing the symbol table to compact; the custom
    // <my-widget>/<data-card> elements carry names that must survive that compaction.
    let html_str = "<html class='root'><body>\
        <p class='drop keep'>x</p>\
        <my-widget><data-card>c</data-card></my-widget></body></html>";
    let dom = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    dom.root()
        .select_css("p")
        .unwrap()
        .first()
        .unwrap()
        .classes_mut()
        .remove(|_| true);

    let html = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        html.contains("<my-widget>"),
        "custom element name must survive symbol compaction: {html}"
    );
    assert!(
        html.contains("<data-card>"),
        "nested custom element name must survive: {html}"
    );
    assert!(
        html.contains("class=\"root\""),
        "the untouched <html> class must survive: {html}"
    );
    assert!(
        !html.contains("drop"),
        "the emptied <p> class is gone: {html}"
    );
}

#[test]
fn repackage_re_derives_overflow_extended_tag_vocab() {
    use std::fmt::Write;
    // 70 distinct custom elements overflow the 63-slot vocab; repackage re-derives the vocab
    // and rewrites the node-index-keyed overflow side map, so every name — vocab and overflow
    // alike — must survive with its node indices remapped (ADR 0002 §4).
    let mut html_str = String::from("<div>");
    for i in 0..70u32 {
        write!(html_str, "<x-{i}>t</x-{i}>").unwrap();
    }
    html_str.push_str("</div>");

    let dom = HtmlDoc::parse(&html_str).unwrap().dom();
    let repacked = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    // A vocab tag, the boundary, and an overflow tag.
    for name in ["<x-0>", "<x-62>", "<x-63>", "<x-69>"] {
        assert!(
            repacked.contains(name),
            "{name} must survive repackage of the overflow vocab: {repacked}"
        );
    }
}

#[test]
fn extended_tags_survive_u16_archived_round_trip() {
    use rkyv::rancor::Error;

    // A small document of custom elements mixed with text/attrs must down-pack to U16 and
    // render identically through the zero-copy archived view (exercising `ExtTagsView`'s
    // archived paths and the `repack` raw-byte copy of the extended tag bytes).
    let html = r#"<my-card id="i" data-x="y"><x-leaf>hi</x-leaf>tail</my-card>"#;
    let dom: DomInner = HtmlDoc::parse(html).unwrap().dom().into_optimal_width();
    let raw = dom.to_html(HtmlFormat::Raw);

    let bytes = rkyv::to_bytes::<Error>(&dom).unwrap();
    let archived = rkyv::access::<ArchivedDomInner, Error>(&bytes[..]).unwrap();
    assert!(
        archived.nodes.view().as_bytes().len().is_multiple_of(15),
        "small document must down-pack to the 15-byte U16 node stride"
    );
    // Render directly off the archived view: `tag_name` resolves through `ExtTagsView::Archived`.
    let archived = archived.bound();
    assert_eq!(
        archived.to_html(HtmlFormat::Raw),
        raw,
        "archived zero-copy render preserves extended tag names"
    );
    assert!(raw.contains("<my-card") && raw.contains("<x-leaf>") && raw.contains("</x-leaf>"));
    // And a custom-element selector resolves against the archived document.
    let leaf: Vec<_> = archived
        .root()
        .select_css("x-leaf")
        .unwrap()
        .map(|el| el.tag_name().to_string())
        .collect();
    assert_eq!(leaf, ["x-leaf"]);
}

#[test]
fn repackage_keeps_svg_subtree_when_classes_compact() {
    // svg/math subtrees are stored as extended elements (ADR 0002 §5); their names share the
    // symbol table with class tokens, so an emptied class (forcing compaction) must not
    // disturb them — and the case is still restored after repackage.
    let html_str = "<html class='root'><body>\
        <p class='drop keep'>x</p>\
        <svg viewBox='0 0 1 1'><clipPath><path d='M0 0'></path></clipPath></svg></body></html>";
    let dom = HtmlDoc::parse(html_str).unwrap().dom_ref_cell();

    dom.root()
        .select_css("p")
        .unwrap()
        .first()
        .unwrap()
        .classes_mut()
        .remove(|_| true);

    let html = HtmlDoc::from(dom.repackage()).to_html(HtmlFormat::Raw);
    assert!(
        html.contains("<clipPath>") && html.contains("</clipPath>"),
        "svg child name and case must survive symbol compaction: {html}"
    );
    assert!(
        html.contains("viewBox=\"0 0 1 1\""),
        "svg attribute name and case must survive: {html}"
    );
    assert!(
        html.contains("<path d=\"M0 0\">"),
        "nested svg element must survive: {html}"
    );
    assert!(
        !html.contains("drop"),
        "the emptied <p> class is gone: {html}"
    );
}
