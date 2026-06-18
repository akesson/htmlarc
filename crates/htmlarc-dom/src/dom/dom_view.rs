use crate::css::{
    AttributeSelector, ClassSelector, ExtTagSelector, IdSelector, ResolvedRef, ResolvedSym,
    ResolvedTag,
};
use crate::dom::NodeIndex;
use crate::dom::nodes::NodesView;
use crate::html::{HtmlAttr, HtmlTag};
use crate::stores::{
    AttrName, AttrStoreView, Attribute, Class, EXT_BASE, EXT_OVERFLOW, ExtTagsView, RunIndex,
    RunValues, RunVecView, StringSource, Sym, SymbolTableView,
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
    pub(crate) attrs: AttrStoreView<'a>,
    pub(crate) symbols: SymbolTableView<'a>,
    pub(crate) class_lists: RunVecView<'a>,
    pub(crate) ext_tags: ExtTagsView<'a>,
    pub(crate) strings: StringSource<'a>,
}

impl<'a> DomView<'a> {
    pub(crate) fn new(
        nodes: NodesView<'a>,
        attrs: AttrStoreView<'a>,
        symbols: SymbolTableView<'a>,
        class_lists: RunVecView<'a>,
        ext_tags: ExtTagsView<'a>,
        strings: StringSource<'a>,
    ) -> Self {
        Self {
            nodes,
            attrs,
            symbols,
            class_lists,
            ext_tags,
            strings,
        }
    }

    /// Advance an externally-held attribute-run cursor (an arena offset, held by the
    /// [`crate::accessors::Attributes`] iterator), dereferencing the next entry into an
    /// [`Attribute`] (its extended name resolved through the symbol table).
    pub(crate) fn next_attr_in_run(&self, offset: &mut Option<u16>) -> Option<Attribute<'a>> {
        let id = self.attrs.next_entry_in_run(offset)?;
        Some(self.attrs.attribute_at(id, self.symbols))
    }

    /// Iterate a node's class list, dereferencing each [`Sym`] through the symbol table
    /// into a borrowed [`Class`]. Used by the formatter and the `classes()` accessor.
    pub(crate) fn class_list_at(&self, start: RunIndex) -> ClassListIter<'a> {
        ClassListIter {
            symbols: self.symbols,
            syms: self.class_lists.run_at(start),
        }
    }

    /// Advance an externally-held class-list cursor (an arena offset, held by the
    /// [`crate::accessors::Classes`] iterator), yielding the next [`Class`].
    pub(crate) fn next_class_in_list(&self, offset: &mut Option<u16>) -> Option<Class<'a>> {
        let val = self.class_lists.next_in_run(offset)?;
        Some(Class(self.symbols.get(Sym(val))))
    }

    /// Iterate a node's attribute run as borrowed [`Attribute`]s, in source order. Used by
    /// the formatter (one run holds std, `data-*`, and unknown attributes alike).
    pub(crate) fn attr_list_at(&self, start: RunIndex) -> AttrListIter<'a> {
        AttrListIter {
            attrs: self.attrs,
            symbols: self.symbols,
            ids: self.attrs.run_at(start),
        }
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

    /// The element's tag name as a string. A standard tag yields its static name; an extended
    /// (custom/unknown) tag — node byte `>= EXT_BASE` — resolves through the `ext_tags` vocab
    /// to the symbol table (ADR 0002 §4). The formatter and `tag_id_class` render through this
    /// so an extended element prints `<my-widget>`, never the `extended` marker.
    pub(crate) fn tag_name(&self, index: NodeIndex) -> &'a str {
        let byte = self.nodes.tag_byte(index);
        if byte >= EXT_BASE {
            self.symbols.get(self.ext_tags.sym_at(index, byte))
        } else {
            HtmlTag::from_repr(byte).unwrap().as_str()
        }
    }

    /// The compound selector's extended-tag check (ADR 0002 §4). `Byte` is a single tag-byte
    /// equality; `OverflowSym` confirms the overflow sentinel then the side-map symbol;
    /// `Absent` never matches (correct through `:not`); `Unresolved` falls back to a string
    /// compare of the node's tag name for the direct entry points that skip the resolve pass.
    pub(crate) fn matches_ext_tag(&self, node: NodeIndex, sel: &ExtTagSelector) -> bool {
        match sel.resolved {
            ResolvedTag::Byte(b) => self.nodes.tag_byte(node) == b,
            ResolvedTag::OverflowSym(sym) => {
                let byte = self.nodes.tag_byte(node);
                byte == EXT_OVERFLOW && self.ext_tags.sym_at(node, byte) == sym
            }
            ResolvedTag::Absent => false,
            ResolvedTag::Unresolved => {
                // The node's stored name is lowercase; a type selector is ASCII-case-
                // insensitive (ADR 0002 §5), so a `clipPath` selector matches `clippath`.
                let byte = self.nodes.tag_byte(node);
                byte >= EXT_BASE && self.tag_name(node).eq_ignore_ascii_case(sel.name)
            }
        }
    }

    /// The compound selector's attribute check (ADR 0002 §3). Each selector takes its
    /// resolved path: `Found(name_sym)` is an integer prefilter over the node's entry names
    /// (only matching-name entries pay the value compare); `Absent` never matches (correct
    /// through `:not`); `Unresolved` falls back to a full string compare for the direct
    /// entry points that skip the resolve pass. A node with no attribute list never matches.
    pub(crate) fn has_attributes(&self, node: NodeIndex, attrs: &[AttributeSelector]) -> bool {
        let Some(start) = self.nodes.attr_list_index(node) else {
            return false;
        };
        attrs.iter().all(|sel| match sel.resolved {
            ResolvedRef::Found(name_sym) => self.attrs.run_at(start).any(|id| {
                self.attrs.name_sym(id) == name_sym
                    && sel.pattern == self.attrs.attribute_at(id, self.symbols)
            }),
            ResolvedRef::Absent => false,
            ResolvedRef::Unresolved => self
                .attrs
                .run_at(start)
                .any(|id| sel.pattern == self.attrs.attribute_at(id, self.symbols)),
        })
    }

    /// The compound selector's `#id` check (ADR 0002 §3). `Found(entry)` is an integer scan
    /// of the node's attribute run for the resolved `(id, value)` entry id; `Absent` never
    /// matches; `Unresolved` falls back to the string [`has_id`](Self::has_id).
    pub(crate) fn has_id_selector(&self, node: NodeIndex, id: &IdSelector) -> bool {
        match id.resolved {
            ResolvedRef::Found(entry) => {
                let Some(start) = self.nodes.attr_list_index(node) else {
                    return false;
                };
                self.attrs.run_at(start).any(|eid| eid == entry)
            }
            ResolvedRef::Absent => false,
            ResolvedRef::Unresolved => self.has_id(node, id.name),
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

    /// The compound selector's class check (ADR 0002 §3). Each selector takes its resolved
    /// path: `Found(sym)` is an integer compare over the node's class syms; `Absent` never
    /// matches (correct through `:not`, which negates the inner result); `Unresolved` falls
    /// back to a string compare for the direct-matching entry points that skip the resolve
    /// pass. A node with no class list never matches.
    pub(crate) fn has_class_selectors(&self, node: NodeIndex, sels: &[ClassSelector]) -> bool {
        let Some(list) = self.nodes.class_list_index(node) else {
            return false;
        };
        sels.iter().all(|sel| match sel.resolved {
            ResolvedSym::Found(sym) => self.class_syms_at(list).any(|v| v == sym),
            ResolvedSym::Absent => false,
            ResolvedSym::Unresolved => self.class_list_at(list).any(|c| *sel == c),
        })
    }

    /// Walk the raw [`Sym`]s of a class list — the integer fast path for matching is a
    /// contiguous scan of the node's run.
    fn class_syms_at(&self, start: RunIndex) -> impl Iterator<Item = Sym> + 'a {
        self.class_lists.run_at(start).map(Sym)
    }

    pub(crate) fn has_id(&self, index: NodeIndex, id: &str) -> bool {
        let Some(start) = self.nodes.attr_list_index(index) else {
            return false;
        };
        self.attrs.run_at(start).any(|entry| {
            let a = self.attrs.attribute_at(entry, self.symbols);
            a.name == AttrName::Std(HtmlAttr::id) && a.val == id
        })
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

/// Iterates a class list's run, dereferencing each stored [`Sym`] into a borrowed
/// [`Class`].
pub(crate) struct ClassListIter<'a> {
    symbols: SymbolTableView<'a>,
    syms: RunValues<'a>,
}

impl<'a> Iterator for ClassListIter<'a> {
    type Item = Class<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let val = self.syms.next()?;
        Some(Class(self.symbols.get(Sym(val))))
    }
}

/// Iterates an attribute list's run, dereferencing each entry id into a borrowed
/// [`Attribute`] (its value from the attr store, its extended name via the symbol table).
pub(crate) struct AttrListIter<'a> {
    attrs: AttrStoreView<'a>,
    symbols: SymbolTableView<'a>,
    ids: RunValues<'a>,
}

impl<'a> Iterator for AttrListIter<'a> {
    type Item = Attribute<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.ids.next()?;
        Some(self.attrs.attribute_at(id, self.symbols))
    }
}
