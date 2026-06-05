use crate::css::AttributeSelector;
use crate::dom::nodes::NodesView;
use crate::html::{HtmlAttr, HtmlTag};
use crate::stores::{
    AttributeStoreView, Class, ClassStoreView, DataAttributeStoreView, StringStackView,
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
    pub(crate) classes: ClassStoreView<'a>,
    pub(crate) strings: StringStackView<'a>,
}

impl<'a> DomView<'a> {
    pub(crate) fn new(
        nodes: NodesView<'a>,
        attrs: AttributeStoreView<'a>,
        dataattrs: DataAttributeStoreView<'a>,
        classes: ClassStoreView<'a>,
        strings: StringStackView<'a>,
    ) -> Self {
        Self {
            nodes,
            attrs,
            dataattrs,
            classes,
            strings,
        }
    }

    /// The text/comment payload of a string node.
    pub(crate) fn string_at(&self, index: u16) -> &'a str {
        let r = self.nodes.text_range(index);
        self.strings.get(r)
    }

    /// The text value if `index` is a text or comment node.
    pub(crate) fn text(&self, index: u16) -> Option<&'a str> {
        let tag = self.nodes.tag(index);
        if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
            Some(self.string_at(index))
        } else {
            None
        }
    }

    pub(crate) fn has_attributes(&self, node: u16, attrs: &[AttributeSelector]) -> bool {
        if let Some(list_index) = self.nodes.attr_list_index(node) {
            attrs
                .iter()
                .all(|a| self.attrs.list_at(list_index).any(|v| *a == v))
        } else {
            false
        }
    }

    pub(crate) fn has_classes<P>(&self, node: u16, classes: &[P]) -> bool
    where
        P: for<'b> PartialEq<Class<'b>>,
    {
        if let Some(list_index) = self.nodes.class_list_index(node) {
            classes
                .iter()
                .all(|c| self.classes.list_at(list_index).any(|v| *c == v))
        } else {
            false
        }
    }

    pub(crate) fn has_data_attributes(&self, node: u16, attrs: &[AttributeSelector]) -> bool {
        if let Some(list_index) = self.nodes.data_attr_list_index(node) {
            attrs
                .iter()
                .all(|a| self.dataattrs.list_at(list_index).any(|v| *a == v))
        } else {
            false
        }
    }

    pub(crate) fn has_id(&self, index: u16, id: &str) -> bool {
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
    pub(crate) fn is_format_inlined(&self, index: u16) -> bool {
        self.is_format_inlined_inner(index, false)
    }

    fn is_format_inlined_inner(&self, index: u16, skip_ancestor_check: bool) -> bool {
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
