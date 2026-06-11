use tinyvec::ArrayVec;

use crate::{
    dom::{DomInner, NodeIndex, Nodes},
    html::{HtmlAttr, HtmlTag},
    stores::{
        AttributeStoreBuilder, DataAttribute, DataAttributeStoreBuilder, ListIndex, RunIndex,
        RunVec, StringStack, SymbolTableBuilder,
    },
};

use super::dom::{DomStack, log, log_list, log_opt_i};

#[derive(Default)]
pub struct DomBuilder {
    pub(crate) nodes: Nodes,
    pub(crate) attrs: AttributeStoreBuilder,
    pub(crate) dataattrs: DataAttributeStoreBuilder,
    pub(crate) symbols: SymbolTableBuilder,
    pub(crate) class_lists: RunVec,
    /// The class-run arena ceiling (one cap covers list count and entries alike — both
    /// are arena slots). Symbol-heap overflow is tracked by `symbols` itself; both are
    /// folded into [`overflow`](Self::overflow).
    class_overflow: Option<&'static str>,
    pub(crate) strings: StringStack,
}

impl DomBuilder {
    pub fn add_text_child(&mut self, tag: HtmlTag, index: NodeIndex, text: &str) -> NodeIndex {
        let range = self.strings.push(text);
        let index = self.nodes.add_as_last_child(index, tag);
        self.nodes.set_text_range(index, range);
        index
    }

    /// Intern a whitespace-separated `class` attribute into the symbol table and write a
    /// run of its tokens (as bare `Sym`s) into the `class_lists` arena, returning the
    /// run's start. On a per-document capacity overflow the builder is poisoned and the
    /// document later discarded; the returned index is then meaningless but never observed.
    pub(crate) fn add_class_list(&mut self, classes: &str) -> RunIndex {
        const OVERFLOW: &str = "class lists exceed 65,535 arena entries";
        let mut names = classes.split_ascii_whitespace();
        let first = self.symbols.intern_or_poison(names.next().unwrap_or(""));
        let Some(start) = self.class_lists.try_new_run(first.as_u16()) else {
            self.class_overflow.get_or_insert(OVERFLOW);
            return RunIndex::from(0);
        };
        for class in names {
            let sym = self.symbols.intern_or_poison(class);
            if !self.class_lists.try_append_last(start, sym.as_u16()) {
                self.class_overflow.get_or_insert(OVERFLOW);
                break;
            }
        }
        start
    }

    pub fn build(self) -> DomInner {
        DomInner {
            nodes: self.nodes,
            attrs: self.attrs.build(),
            dataattrs: self.dataattrs.build(),
            symbols: self.symbols.build(),
            class_lists: self.class_lists,
            strings: self.strings,
        }
    }

    /// The first per-document capacity overflow reported by any sub-store builder, if
    /// any. The node/depth overflow tracked by [`DomBuilderCursor`] is folded in there.
    pub fn overflow(&self) -> Option<&'static str> {
        self.attrs
            .overflow()
            .or(self.symbols.overflow())
            .or(self.class_overflow)
            .or_else(|| self.dataattrs.overflow())
    }
}

/// Maximum element nesting depth. Past this the builder poisons the document and skips
/// the over-deep subtree rather than panicking the fixed-capacity parse stacks. General
/// scraped HTML reaches well past the previous limit of 64 (deep `<div>`/`<span>` soup),
/// so this is set generously; the cost is `256 * (1 + 4)` bytes of stack per parse.
///
/// TODO(ADR 0002): the general-web gate found 0.23% of Common Crawl docs deeper than 256
/// (max 2,950). Those are skipped cleanly today; the redesign should switch these
/// `ArrayVec` stacks to a heap `Vec` with a higher sanity cap (~8,192) — an `ArrayVec` that
/// large is too much stack per parse.
const MAX_DEPTH: usize = 256;

/// Maximum node count, matching the U24 node-index sentinel (`Nodes` are always built at
/// U24 width during parsing — see `Nodes::new`). Past this the builder poisons the
/// document instead of tripping `Nodes::add_node`'s assert and aborting the import.
const MAX_NODES: usize = 0x00FF_FFFF;

#[derive(Default)]
pub struct DomBuilderCursor {
    pub dom: DomBuilder,
    pub tag_stack: ArrayVec<[HtmlTag; MAX_DEPTH]>,
    pub index_stack: ArrayVec<[NodeIndex; MAX_DEPTH]>,
    pub attr_list_index: Option<ListIndex>,
    pub dataattr_list_index: Option<ListIndex>,
    /// Set (first reason wins) when the node count or nesting depth overflows; combined
    /// with the sub-store builders' flags by [`overflow`](Self::overflow).
    overflow: Option<&'static str>,
}

impl DomBuilderCursor {
    fn index(&self) -> NodeIndex {
        *self.index_stack.last().unwrap_or(&NodeIndex::ROOT)
    }
    fn push_index(&mut self, index: NodeIndex) {
        self.index_stack.push(index)
    }

    /// The first per-document capacity overflow reason, across the cursor's own
    /// node/depth guard and every sub-store builder. `Some` means the document must be
    /// discarded — its partially built state is intentionally inconsistent.
    pub fn overflow(&self) -> Option<&'static str> {
        self.overflow.or_else(|| self.dom.overflow())
    }

    /// Whether another node can be added without exceeding [`MAX_NODES`]; records the
    /// overflow (once) when it cannot.
    fn node_budget_ok(&mut self) -> bool {
        if self.dom.nodes.len() >= MAX_NODES {
            self.overflow
                .get_or_insert("document exceeds 16,777,215 nodes");
            false
        } else {
            true
        }
    }
}

impl DomStack for DomBuilderCursor {
    fn _push_tag(&mut self, tag: HtmlTag) {
        // Over-deep or over-large documents are poisoned and the offending node skipped.
        // Both stacks are left untouched (they stay in lock-step), so the matching close
        // tag still pops cleanly — the document is discarded by `HtmlDoc::parse` anyway.
        if self.tag_stack.len() >= MAX_DEPTH {
            self.overflow.get_or_insert("element nesting exceeds 256");
            return;
        }
        if !self.node_budget_ok() {
            return;
        }
        self.tag_stack.push(tag);
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        let i = self.dom.nodes.add_as_last_child(self.index(), tag);
        log(i, || format!("push: {tag}"));
        self.push_index(i);
    }

    fn stack_info(&self) -> String {
        self.tag_stack
            .iter()
            .map(HtmlTag::as_str)
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn _last_tag(&mut self) -> HtmlTag {
        self.tag_stack.last().copied().unwrap_or(HtmlTag::sys_root)
    }

    fn _pop_tag(&mut self) -> Option<HtmlTag> {
        let i = self.index_stack.pop();
        let tag = self.tag_stack.pop();
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        log_opt_i(i, || format!("pop: {tag:?}"));
        tag
    }

    fn add_text_tag(&mut self, tag: HtmlTag, text: &str) {
        if !self.node_budget_ok() {
            return;
        }
        let index = self.index();
        self.attr_list_index = None;
        self.dataattr_list_index = None;
        log(index, || format!("add text: {:?}", text));
        self.dom.add_text_child(tag, index, text);
    }

    fn add_attribute_and_value(&mut self, tag: HtmlAttr, val: &str) {
        let index = self.index();
        if tag == HtmlAttr::class {
            log_list(index, Some(""), || format!("add class={val}"));
            let list_index = self.dom.add_class_list(val);
            self.dom
                .nodes
                .set_class_list_index(index, Some(list_index.as_u16()));
        } else if let Some(list_index) = self.attr_list_index {
            self.dom.attrs.add_attribute(list_index, tag, val);
        } else {
            let list_index = self.dom.attrs.new_list(tag, val);
            self.attr_list_index = Some(list_index);
            self.dom
                .nodes
                .set_attr_list_index(index, Some(list_index.as_u16()));
        }
    }

    fn add_data_attribute(&mut self, tag: &str, val: &str) {
        let index = self.index();

        let data_attr = DataAttribute { tag, val };

        if let Some(list_index) = self.dataattr_list_index {
            self.dom.dataattrs.add_attribute(list_index, &data_attr);
        } else {
            let list_index = self.dom.dataattrs.add_list(&data_attr);
            self.dataattr_list_index = Some(list_index);
            self.dom
                .nodes
                .set_data_attr_list_index(index, Some(list_index.as_u16()));
        }
    }
}

#[cfg(test)]
fn dbg_class_list(dom: &DomInner, index: RunIndex) -> String {
    dom.view()
        .class_list_at(index)
        .map(|c| c.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

// Token order within a class list is preserved (the symbol table dedups names but the
// list keeps each token's first-seen position). Ported from the former ClassStoreBuilder.
#[test]
fn class_list_preserves_token_order() {
    let mut b = DomBuilder::default();
    let l1 = b.add_class_list("one a two");
    let l2 = b.add_class_list("a one b");
    let dom = b.build();
    assert_eq!(dbg_class_list(&dom, l1), "one, a, two");
    assert_eq!(dbg_class_list(&dom, l2), "a, one, b");
}
