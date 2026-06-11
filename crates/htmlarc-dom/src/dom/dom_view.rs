use crate::css::AttributeSelector;
use crate::dom::NodeIndex;
use crate::dom::nodes::NodesView;
use crate::html::{HtmlAttr, HtmlTag};
use crate::stores::{
    AttributeStoreView, Class, DataAttributeStoreView, ListIndex, ListVecView, StringStackView,
    Sym, SymbolTableView,
};

/// A borrowed, read-only view over a DOM document.
///
/// `DomView` is the single currency the query layer (iterators, CSS selectors, the
/// formatter, [`crate::html::HtmlElement`]) reads through. It is produced both by the
/// owned [`super::DomInner`] (borrowing its `Vec`s as slices) and by the rkyv-archived
/// `ArchivedDomInner` (borrowing the mmap'd `ArchivedVec`s) — so the exact same query
/// code runs against an in-memory document and a zero-copy memory-mapped one.
///
/// Its fields mirror `DomInner`'s, and each sub-view mirrors the owned store's read
/// API, so call sites are representation-agnostic.
#[derive(Clone, Copy)]
pub struct DomView<'a> {
    pub(crate) nodes: NodesView<'a>,
    pub(crate) attrs: AttributeStoreView<'a>,
    pub(crate) dataattrs: DataAttributeStoreView<'a>,
    pub(crate) symbols: SymbolTableView<'a>,
    pub(crate) class_lists: ListVecView<'a>,
    pub(crate) strings: StringStackView<'a>,
}

impl<'a> DomView<'a> {
    pub(crate) fn new(
        nodes: NodesView<'a>,
        attrs: AttributeStoreView<'a>,
        dataattrs: DataAttributeStoreView<'a>,
        symbols: SymbolTableView<'a>,
        class_lists: ListVecView<'a>,
        strings: StringStackView<'a>,
    ) -> Self {
        Self {
            nodes,
            attrs,
            dataattrs,
            symbols,
            class_lists,
            strings,
        }
    }

    /// Iterate a node's class list, dereferencing each [`Sym`] through the symbol table
    /// into a borrowed [`Class`]. Used by the formatter and the `classes()` accessor.
    pub(crate) fn class_list_at(&self, index: ListIndex) -> ClassListIter<'a> {
        ClassListIter {
            symbols: self.symbols,
            lists: self.class_lists,
            index: self.class_lists.head_index_at(index),
        }
    }

    /// Advance an externally-held class-list cursor (the [`crate::accessors::Classes`]
    /// iterator), yielding the next [`Class`].
    pub(crate) fn next_class_in_list(&self, index: &mut Option<ListIndex>) -> Option<Class<'a>> {
        let i = (*index)?;
        let (next, val) = self.class_lists.next(i);
        *index = next;
        Some(Class(self.symbols.get(Sym(val))))
    }

    /// The text/comment payload of a string node.
    pub(crate) fn string_at(&self, index: NodeIndex) -> &'a str {
        let r = self.nodes.text_range(index);
        self.strings.get(r)
    }

    /// The text value if `index` is a text or comment node.
    pub(crate) fn text(&self, index: NodeIndex) -> Option<&'a str> {
        let tag = self.nodes.tag(index);
        if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
            Some(self.string_at(index))
        } else {
            None
        }
    }

    pub(crate) fn has_attributes(&self, node: NodeIndex, attrs: &[AttributeSelector]) -> bool {
        if let Some(list_index) = self.nodes.attr_list_index(node) {
            attrs
                .iter()
                .all(|a| self.attrs.list_at(list_index).any(|v| *a == v))
        } else {
            false
        }
    }

    pub(crate) fn has_classes<P>(&self, node: NodeIndex, classes: &[P]) -> bool
    where
        P: for<'b> PartialEq<Class<'b>>,
    {
        if let Some(list_index) = self.nodes.class_list_index(node) {
            classes
                .iter()
                .all(|c| self.class_list_at(list_index).any(|v| *c == v))
        } else {
            false
        }
    }

    pub(crate) fn has_data_attributes(&self, node: NodeIndex, attrs: &[AttributeSelector]) -> bool {
        if let Some(list_index) = self.nodes.data_attr_list_index(node) {
            attrs
                .iter()
                .all(|a| self.dataattrs.list_at(list_index).any(|v| *a == v))
        } else {
            false
        }
    }

    pub(crate) fn has_id(&self, index: NodeIndex, id: &str) -> bool {
        if let Some(list_index) = self.nodes.attr_list_index(index) {
            self.attrs
                .list_at(list_index)
                .any(|v| v.tag == HtmlAttr::id && v.val == id)
        } else {
            false
        }
    }

    /// Whether the subtree rooted at `index` can be rendered inline (no inserted
    /// whitespace). Mirrors `HtmlElement::is_format_inlined`, but walks the node blob
    /// directly so the formatter needs only a [`DomView`], not a `DomRead` cursor.
    pub(crate) fn is_format_inlined(&self, index: NodeIndex) -> bool {
        self.is_format_inlined_inner(index, false)
    }

    fn is_format_inlined_inner(&self, index: NodeIndex, skip_ancestor_check: bool) -> bool {
        let tag = self.nodes.tag(index);
        match tag {
            HtmlTag::DOCTYPE => false,
            HtmlTag::sys_text | HtmlTag::sys_comment => true,
            _ => {
                if !skip_ancestor_check {
                    let mut ancestor = self.nodes.parent_index(index);
                    while let Some(a) = ancestor {
                        if self.nodes.tag(a) == HtmlTag::noscript {
                            return true;
                        }
                        ancestor = self.nodes.parent_index(a);
                    }
                }
                if !tag.is_format_inlined() {
                    return false;
                }
                let mut child = self.nodes.first_child_index(index);
                while let Some(c) = child {
                    if !self.is_format_inlined_inner(c, true) {
                        return false;
                    }
                    child = self.nodes.next_sibling_index(c);
                }
                true
            }
        }
    }
}

/// Iterates a class list, dereferencing each stored [`Sym`] into a borrowed [`Class`].
/// Replaces the old `ClassListView`; the symbol indirection is the only change.
pub(crate) struct ClassListIter<'a> {
    symbols: SymbolTableView<'a>,
    lists: ListVecView<'a>,
    index: Option<ListIndex>,
}

impl<'a> Iterator for ClassListIter<'a> {
    type Item = Class<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.index?;
        let (next, val) = self.lists.next(index);
        self.index = next;
        Some(Class(self.symbols.get(Sym(val))))
    }
}
