use crate::{
    accessors::{Attributes, AttributesMut, Classes, ClassesMut},
    css::{self, AttributeSelector, ParseError, Selector, SelectorList},
    dom::{DomRead, DomRef, DomRefCell, DomView, NodeIndex, Nodes, NodesView},
    error::ElementError,
    fmt::HtmlFormat,
    html::HtmlTag,
    iters::{DomIterator, ElementIter, MatchIter, RelativeIter, RevElementIter},
    stores::{AttrName, Class},
};

use super::HtmlAttr;

pub(crate) const IGNORE_TAGS: &[HtmlTag] =
    &[HtmlTag::sys_text, HtmlTag::sys_comment, HtmlTag::sys_root];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtmlElement<'dom, Dom> {
    pub(crate) dom: &'dom Dom,
    pub(crate) index: NodeIndex,
}

impl<'dom, Dom: DomRead> HtmlElement<'dom, Dom> {
    pub fn new(dom: &'dom Dom, index: NodeIndex) -> Self {
        Self { dom, index }
    }

    pub fn root(dom: &'dom Dom) -> Self {
        Self {
            dom,
            index: NodeIndex::ROOT,
        }
    }

    pub fn dom(&self) -> &'dom Dom {
        self.dom
    }

    fn with_nodes<R, F: Fn(NodesView) -> R>(&self, f: F) -> R {
        self.dom.with_nodes(f)
    }

    fn with_nodes_new<F: Fn(NodesView) -> Option<NodeIndex>>(&self, f: F) -> Option<Self> {
        self.dom
            .with_nodes(|nodes| f(nodes).map(|index| self.new_with_index(index)))
    }

    fn with_view<R, F: Fn(DomView) -> R>(&self, f: F) -> R {
        self.dom.with_view(f)
    }

    pub fn index(&self) -> NodeIndex {
        self.index
    }

    fn new_with_index(&self, index: NodeIndex) -> Self {
        Self::new(self.dom, index)
    }

    fn cloned(&self) -> Self {
        Self::new(self.dom, self.index)
    }

    pub fn parent(&self) -> Result<Self, ElementError<'dom, Dom>> {
        // A parent can by definition not be a text or comment node,
        // so no need to check for that here
        self.with_nodes_new(|nodes| nodes.parent_index(self.index))
            .ok_or_else(|| ElementError::NoParent(self.cloned()))
    }

    pub fn ancestors(&self) -> RelativeIter<'dom, Dom> {
        RelativeIter::ancestors(self)
    }

    pub fn prev_sibling(&self) -> Result<Self, ElementError<'dom, Dom>> {
        self.prev_siblings()
            .next()
            .ok_or_else(|| ElementError::NoPreviousSibling(self.cloned()))
    }

    pub fn prev_siblings(&self) -> RelativeIter<'dom, Dom> {
        RelativeIter::prev_siblings(self)
    }

    pub(crate) fn prev_sibling_all(&self) -> Option<Self> {
        self.with_nodes_new(|nodes| nodes.prev_sibling_index(self.index))
    }

    pub fn next_sibling(&self) -> Result<Self, ElementError<'dom, Dom>> {
        self.next_siblings()
            .next()
            .ok_or_else(|| ElementError::NoNextSibling(self.cloned()))
    }

    pub fn next_siblings(&self) -> RelativeIter<'dom, Dom> {
        RelativeIter::next_siblings(self)
    }

    pub(crate) fn next_sibling_all(&self) -> Option<Self> {
        self.with_nodes_new(|nodes| nodes.next_sibling_index(self.index))
    }

    pub fn first_child(&self) -> Result<Self, ElementError<'dom, Dom>> {
        self.children()
            .next()
            .ok_or_else(|| ElementError::NoChild(self.cloned()))
    }

    pub fn path<I: IntoIterator<Item = HtmlTag>>(
        &self,
        path: I,
    ) -> Result<Self, ElementError<'dom, Dom>> {
        let mut el = self.cloned();
        for tag in path {
            el = el.first_child_tag(tag)?;
        }
        Ok(el)
    }
    pub fn first_child_tag(&self, tag: HtmlTag) -> Result<Self, ElementError<'dom, Dom>> {
        self.children()
            .find(|el| el.tag() == tag)
            .ok_or_else(|| ElementError::NoChild(self.cloned()))
    }

    pub fn child(&self, selector: SelectorList<'dom>) -> Result<Self, ElementError<'dom, Dom>> {
        self.select_child(selector)
            .next()
            .ok_or_else(|| ElementError::NoChild(self.cloned()))
    }

    pub fn children(&self) -> RelativeIter<'dom, Dom> {
        RelativeIter::children(self)
    }

    pub(crate) fn first_child_all(&self) -> Option<Self> {
        self.with_nodes_new(|nodes| nodes.first_child_index(self.index))
    }

    #[allow(unused)]
    pub(crate) fn last_child_all(&self) -> Option<Self> {
        self.with_nodes_new(|nodes| nodes.last_child_index(self.index))
    }

    pub fn is_block(&self) -> bool {
        self.with_nodes(|nodes| nodes.is_block_element(self.index()))
    }

    pub fn is_format_inlined(&self) -> bool {
        self.is_format_inlined_inner(false)
    }
    fn is_format_inlined_inner(&self, skip_ancestor_check: bool) -> bool {
        let tag = self.tag();
        match tag {
            HtmlTag::DOCTYPE => false,
            HtmlTag::sys_text | HtmlTag::sys_comment => true,
            _ => {
                if !skip_ancestor_check {
                    // check if we are inside a noscript tag
                    for parent in RelativeIter::ancestors(self) {
                        if parent.tag() == HtmlTag::noscript {
                            return true;
                        }
                    }
                }

                tag.is_format_inlined()
                    && RelativeIter::children(self).all(|child| child.is_format_inlined_inner(true))
            }
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn tag(&self) -> HtmlTag {
        self.with_nodes(|nodes| nodes.tag(self.index()))
    }

    /// Returns the text value if the element is a text or comment node
    pub fn text(&self) -> Option<String> {
        self.dom
            .with_view(|view| view.text(self.index()).map(|s| s.to_string()))
    }

    /// Gathers the text content of the element and its descendants.
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        self.for_each_text_chunk(|chunk| out.push_str(chunk));
        out
    }

    /// Streams the element's text content to `f` as borrowed per-text-node slices, in document
    /// order — the zero-allocation counterpart to [`text_content`](Self::text_content).
    /// Concatenating the slices reproduces `text_content`'s string exactly, but without the
    /// intermediate `String` per text node or the char-by-char collection. Each slice borrows the
    /// backing store only for the duration of the call, so `f` must copy whatever it keeps.
    pub fn for_each_text_chunk(&self, mut f: impl FnMut(&str)) {
        for el in self.descendants().set_include_text() {
            if el.tag() == HtmlTag::sys_text {
                el.dom.with_view(|view| {
                    if let Some(s) = view.text(el.index()) {
                        f(s);
                    }
                });
            }
        }
    }

    pub fn count_parents(&self) -> u16 {
        self.ancestors().count() as u16
    }

    /// Render this element and its descendants (its "outer HTML"). On the root element this is
    /// the whole document — the same output as [`DomRead::to_html`] on the backing.
    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        self.with_view(|view| fmt.to_html(view, self.index()))
    }

    pub fn writeable<'a>(&self, dom: &'a DomRefCell) -> HtmlElement<'a, DomRefCell> {
        HtmlElement::new(dom, self.index())
    }

    /// Iterates this element's descendants in document order. The iterator is the backing's choice
    /// ([`DomRead::Descendants`]): the layout-exploiting [`LinearSweep`](crate::iters::LinearSweep)
    /// for immutable/contiguous backings (mmap, fresh-parsed `DomInner`), the tree-walking
    /// [`ElementIter`] for the mutable `DomRefCell`. Same element stream either way.
    pub fn descendants(&self) -> Dom::Descendants<'dom> {
        self.dom.descendants_from(self.index)
    }

    /// Iterates from this element to the end of the document in document order (so it also yields
    /// the element's later siblings, its parent's later siblings, etc.). Backing-chosen iterator —
    /// see [`descendants`](Self::descendants).
    pub fn forwards(&self) -> Dom::Forward<'dom> {
        self.dom.forward_from(self.index)
    }

    /// Like [`descendants`](Self::descendants) but always the tree-walking [`ElementIter`], even on
    /// a contiguous backing. The mutation-safe, dead-slot-skipping walk — used by `rebuild` (whose
    /// input may contain dead slots) and as the A/B counterpart in the iterator benchmark.
    ///
    /// Not part of the advertised API (`#[doc(hidden)]`): on an immutable backing
    /// [`descendants`](Self::descendants) is already correct *and* faster, and on `DomRefCell` it is
    /// already the tree-walk — so normal callers never need this. It stays `pub` only so the
    /// out-of-crate benchmark can force the tree-walk to isolate the iterator cost.
    #[doc(hidden)]
    pub fn descendants_walk(&self) -> ElementIter<'dom, Dom> {
        ElementIter::descendants_at(self.dom, self.index)
    }

    /// Like [`forwards`](Self::forwards) but always the tree-walking [`ElementIter`] — see
    /// [`descendants_walk`](Self::descendants_walk) for why this is `#[doc(hidden)]`.
    #[doc(hidden)]
    pub fn forwards_walk(&self) -> ElementIter<'dom, Dom> {
        ElementIter::forwards_at(self.dom, self.index)
    }

    pub fn reverse(&self) -> RevElementIter<'dom, Dom> {
        RevElementIter::reverse(self)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn has_attributes(&self, attrs: &[AttributeSelector]) -> bool {
        self.with_view(|view| view.has_attributes(self.index(), attrs))
    }

    pub fn attribute(&self, attr: HtmlAttr) -> Option<String> {
        self.find_attribute(attr, |v| v.map(|s| s.to_string()))
    }

    /// The attribute value for `name`, looked up by string — standard names resolve to their
    /// [`HtmlAttr`], anything else (`data-*`, custom) matches the extended names. ASCII
    /// case-insensitive on both paths, like HTML attribute names themselves.
    pub fn get_attribute(&self, name: &str) -> Option<String> {
        self.with_attribute(name, |val| val.map(str::to_string))
    }

    /// Runs `f` on the borrowed value of the attribute named `name` (`None` when absent) — the
    /// zero-allocation counterpart to [`get_attribute`](Self::get_attribute), with the identical
    /// string-name resolution (standard names via [`HtmlAttr`], everything else matched against the
    /// extended names, ASCII case-insensitive). The value borrows the backing store only for the
    /// duration of the call.
    pub fn with_attribute<R>(&self, name: &str, f: impl FnOnce(Option<&str>) -> R) -> R {
        use std::str::FromStr;
        let target = HtmlAttr::from_str(name).map_or(AttrName::Ext(name), AttrName::Std);
        self.dom.with_view(|view| {
            let val = view
                .nodes
                .attr_list_index(self.index())
                .and_then(|idx| {
                    view.attr_list_at(idx).find(|a| match (a.name, target) {
                        (AttrName::Ext(stored), AttrName::Ext(wanted)) => {
                            stored.eq_ignore_ascii_case(wanted)
                        }
                        _ => a.name == target,
                    })
                })
                .map(|a| a.val);
            f(val)
        })
    }

    /// Whether an attribute named `name` is present (ASCII case-insensitive), without
    /// materializing its value — the presence-only counterpart to [`get_attribute`].
    pub fn has_attribute(&self, name: &str) -> bool {
        use std::str::FromStr;
        let target = HtmlAttr::from_str(name).map_or(AttrName::Ext(name), AttrName::Std);
        self.with_view(|view| {
            view.nodes.attr_list_index(self.index()).is_some_and(|idx| {
                view.attr_list_at(idx).any(|a| match (a.name, target) {
                    (AttrName::Ext(stored), AttrName::Ext(wanted)) => {
                        stored.eq_ignore_ascii_case(wanted)
                    }
                    _ => a.name == target,
                })
            })
        })
    }

    pub fn find_attribute<R, F: Fn(Option<&str>) -> R>(&self, tag: HtmlAttr, f: F) -> R {
        self.with_view(|view| {
            let val = view
                .nodes
                .attr_list_index(self.index())
                .and_then(|idx| {
                    view.attr_list_at(idx)
                        .find(|a| a.name == AttrName::Std(tag))
                })
                .map(|a| a.val);
            f(val)
        })
    }

    pub fn with_id<R, F: Fn(Option<&str>) -> R>(&self, f: F) -> R {
        self.find_attribute(HtmlAttr::id, f)
    }

    pub fn has_id(&self, id: &str) -> bool {
        self.with_view(|view| view.has_id(self.index(), id))
    }

    pub fn has_classes<P>(&self, classes: &[P]) -> bool
    where
        P: for<'a> PartialEq<Class<'a>>,
    {
        self.with_view(move |view| view.has_classes(self.index(), classes))
    }

    pub fn is_root(&self) -> bool {
        self.index() == NodeIndex::ROOT
    }

    pub fn has_no_children(&self) -> bool {
        RelativeIter::children(self).next().is_none()
    }

    pub fn is_first_child(&self) -> bool {
        self.nth_position(|_| true) == 1
    }

    pub fn is_last_child(&self) -> bool {
        self.nth_reverse_position(|_| true) == 1
    }

    pub fn is_first_of_type(&self) -> bool {
        self.nth_position(|el| el.tag() == self.tag()) == 1
    }

    pub fn is_last_of_type(&self) -> bool {
        self.nth_reverse_position(|el| el.tag() == self.tag()) == 1
    }

    pub fn is_only_of_type(&self) -> bool {
        self.is_first_of_type() && self.is_last_of_type()
    }

    pub(crate) fn nth_position<F>(&self, count_condition: F) -> usize
    where
        F: Fn(&HtmlElement<'_, Dom>) -> bool,
    {
        RelativeIter::prev_siblings(self).fold(
            1,
            |p, el| {
                if count_condition(&el) { p + 1 } else { p }
            },
        )
    }

    pub(crate) fn nth_reverse_position<F>(&self, count_condition: F) -> usize
    where
        F: Fn(&HtmlElement<'_, Dom>) -> bool,
    {
        RelativeIter::next_siblings(self).fold(
            1,
            |p, el| {
                if count_condition(&el) { p + 1 } else { p }
            },
        )
    }

    /// Selects matching descendants in document order, driven by the backing's forward iterator
    /// (so `select` over an immutable backing automatically uses the fast linear walk).
    pub fn select(&self, selector: SelectorList<'dom>) -> MatchIter<'dom, Dom, Dom::Forward<'dom>> {
        MatchIter::new(self.forwards(), selector)
    }

    /// [`select`](Self::select) forced onto the tree-walking [`ElementIter`] — the A/B counterpart
    /// for benchmarking the linear `select`. Not advertised API; see
    /// [`forwards_walk`](Self::forwards_walk).
    #[doc(hidden)]
    pub fn select_walk(
        &self,
        selector: SelectorList<'dom>,
    ) -> MatchIter<'dom, Dom, ElementIter<'dom, Dom>> {
        MatchIter::new(self.forwards_walk(), selector)
    }

    pub fn select_child(
        &self,
        selector: SelectorList<'dom>,
    ) -> MatchIter<'dom, Dom, RelativeIter<'dom, Dom>> {
        MatchIter::new(RelativeIter::children(self), selector)
    }

    pub fn select_css(
        &self,
        selector: &'dom str,
    ) -> Result<MatchIter<'dom, Dom, Dom::Forward<'dom>>, ParseError> {
        let selector = css::parse_css(selector)?;
        Ok(MatchIter::new(self.forwards(), selector))
    }

    /// Whether this element matches the (pre-parsed) selector.
    pub fn matches(&self, selector: &SelectorList<'dom>) -> bool {
        selector.matches(self)
    }

    /// Parse `selector` and report whether this element matches it.
    pub fn matches_css(&self, selector: &str) -> Result<bool, ParseError> {
        Ok(css::parse_css(selector)?.matches(self))
    }
}

impl<'dom, Dom: DomRef> HtmlElement<'dom, Dom> {
    pub fn id(&self) -> Option<&str> {
        self.attributes()
            .find(|attr| attr.name == AttrName::Std(HtmlAttr::id))
            .map(|attr| attr.val)
    }

    /// The element's tag name as a string — a standard tag's static name, or an extended
    /// (custom/unknown) element's real name resolved through the per-document vocab (ADR 0002
    /// §4). Unlike [`tag`](Self::tag), which returns `HtmlTag::extended` for custom elements,
    /// this never yields the `extended` marker spelling.
    pub fn tag_name(&self) -> &'dom str {
        self.dom.dom_view().tag_name(self.index())
    }

    pub fn tag_id_class(&self) -> String {
        let mut tag_id_class = self.tag_name().to_string();
        if let Some(id) = self.id() {
            tag_id_class.push_str(&format!("#{}", id));
        }
        for class in self.classes() {
            tag_id_class.push_str(&format!(".{}", class));
        }
        tag_id_class
    }

    /// The element's location as a `tag#id.class` chain from just under the root down to the
    /// element itself, joined with `" > "` — e.g. `body > main > article#a.x`. Empty for the
    /// root itself.
    pub fn css_path(&self) -> String {
        let mut segs: Vec<String> = std::iter::once(self.cloned())
            .chain(self.ancestors())
            .filter(|el| !el.is_root())
            .map(|el| el.tag_id_class())
            .collect();
        segs.reverse();
        segs.join(" > ")
    }

    pub fn attributes(&self) -> Attributes<'dom, Dom> {
        Attributes {
            dom: self.dom,
            index: self
                .with_nodes(|nodes| nodes.attr_list_index(self.index()))
                .map(|start| start.as_u16()),
        }
    }

    /// Obtain a read lock for the classes.
    pub fn classes(&self) -> Classes<'dom, Dom> {
        let index = self.with_nodes(|nodes| nodes.class_list_index(self.index));
        Classes {
            dom: self.dom,
            index: index.map(|start| start.as_u16()),
        }
    }
}

impl<'dom> HtmlElement<'dom, DomRefCell> {
    fn edit<F: Fn(&mut Nodes) -> NodeIndex>(&self, f: F) -> Self {
        self.dom
            .with_mut(|dom| self.new_with_index(f(&mut dom.nodes)))
    }

    pub fn insert_sibling_after(&self, tag: HtmlTag) -> Self {
        self.edit(|nodes| nodes.add_as_next_sibling(self.index(), tag))
    }

    pub fn insert_sibling_before(&self, tag: HtmlTag) -> Self {
        self.edit(|nodes| nodes.add_as_prev_sibling(self.index(), tag))
    }

    pub fn prepend_child(&self, tag: HtmlTag) -> Self {
        self.edit(|nodes| nodes.add_as_first_child(self.index(), tag))
    }

    pub fn append_child(&self, tag: HtmlTag) -> Self {
        self.edit(|nodes| nodes.add_as_last_child(self.index(), tag))
    }

    pub fn append_text_child(&self, text: &str) -> NodeIndex {
        self.dom
            .with_mut(|dom| dom.append_text_child(HtmlTag::sys_text, self.index(), text))
    }

    pub fn replace_text(&self, text: &str) {
        self.dom
            .with_mut(|dom| dom.replace_text(self.index(), text))
    }

    pub fn append_comment_child(&self, text: &str) -> NodeIndex {
        self.dom
            .with_mut(|dom| dom.append_text_child(HtmlTag::sys_comment, self.index(), text))
    }
    /// Removes the current node from the tree
    /// The cursor is moved to the previous sibling if it exists, otherwise to the next sibling
    pub fn remove(&self) -> Option<Self> {
        self.dom
            .with_mut(|dom| dom.remove(self.index()))
            .map(|index| self.new_with_index(index))
    }

    /// Unwraps the current node, moving its children to its parent
    /// The cursor is moved to the next sibling if it exists, otherwise to the previous sibling
    pub fn unwrap_element(&self) -> Option<Self> {
        let index = self.index();
        self.dom
            .with_mut(|dom| dom.unwrap_element(index))
            .map(|index| self.new_with_index(index))
    }

    /// Replaces the current node with another one from the tree,
    pub fn replace_with(&self, new_index: NodeIndex) -> Option<Self> {
        let index = self.index();
        self.dom.with_mut(|dom| dom.replace_with(index, new_index));

        Some(self.new_with_index(new_index))
    }

    pub fn remove_children(&self) {
        self.dom
            .with_mut(|dom| dom.nodes.remove_children(self.index()));
    }

    pub fn attributes_mut(&self) -> AttributesMut<'dom> {
        let index = self.with_nodes(|nodes| nodes.attr_list_index(self.index));
        AttributesMut {
            lock: self.dom.mut_handle(),
            node: self.index,
            index,
        }
    }

    pub fn classes_mut(&self) -> ClassesMut<'dom> {
        let index = self.with_nodes(|nodes| nodes.class_list_index(self.index));
        ClassesMut {
            lock: self.dom.mut_handle(),
            node: self.index,
            index,
        }
    }

    #[track_caller]
    pub fn log<S: ToString, F: FnOnce() -> S>(&self, f: F) {
        fn type_name_of<T>(_: T) -> &'static str {
            std::any::type_name::<T>()
        }
        //Ex: &html_doc::doc::tests::logging::{{closure}}
        let name = type_name_of(&f);
        let function = &name[1..name.len() - 13];

        // kept if there's a need to log the file of where it is logged as well.
        // let caller = Location::caller();
        // let file = caller.file();
        let log_comment = self.insert_sibling_before(HtmlTag::sys_comment);
        let string = format!(" [{function}]\n{} ", f().to_string());
        self.dom
            .with_mut(|dom| dom.replace_text(log_comment.index(), &string));
    }
}

#[test]
fn get_attribute_by_string_name() {
    const HTML: &str = r#"<body><a href="/x" data-k="v" wonky="yes" aria-label="lbl">t</a></body>"#;
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let a = dom.root().select_css("a").unwrap().next().unwrap();

    // Standard attribute, exact and case-insensitive (incl. kebab-case names).
    assert_eq!(a.get_attribute("href").as_deref(), Some("/x"));
    assert_eq!(a.get_attribute("HREF").as_deref(), Some("/x"));
    assert_eq!(a.get_attribute("aria-label").as_deref(), Some("lbl"));
    // Extended (data-* / unknown) attributes, case-insensitive.
    assert_eq!(a.get_attribute("data-k").as_deref(), Some("v"));
    assert_eq!(a.get_attribute("DATA-K").as_deref(), Some("v"));
    assert_eq!(a.get_attribute("wonky").as_deref(), Some("yes"));
    // Absent.
    assert_eq!(a.get_attribute("title"), None);
    assert_eq!(a.get_attribute("data-missing"), None);

    // has_attribute mirrors get_attribute presence without materializing the value.
    assert!(a.has_attribute("href"));
    assert!(a.has_attribute("HREF"));
    assert!(a.has_attribute("aria-label"));
    assert!(a.has_attribute("data-k"));
    assert!(a.has_attribute("DATA-K"));
    assert!(a.has_attribute("wonky"));
    assert!(!a.has_attribute("title"));
    assert!(!a.has_attribute("data-missing"));
}

#[test]
fn for_each_text_chunk_matches_text_content() {
    const HTML: &str =
        r#"<body><article>One <b>two</b><!-- skip --> A&amp;B<p>tail</p></article></body>"#;
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let article = dom.root().select_css("article").unwrap().next().unwrap();

    // Concatenating the borrowed chunks reproduces text_content exactly — comments excluded,
    // entities already decoded — with no intermediate per-node String.
    let mut chunked = String::new();
    let mut n_chunks = 0;
    article.for_each_text_chunk(|s| {
        chunked.push_str(s);
        n_chunks += 1;
    });
    assert_eq!(chunked, article.text_content());
    assert!(chunked.contains("A&B"));
    assert!(!chunked.contains("skip"));
    // Text arrives as separate node-sized chunks, not one pre-merged string.
    assert!(n_chunks >= 2);

    // A leaf element with no text descendants yields no chunks (and an empty text_content).
    let b = dom.root().select_css("p").unwrap().next().unwrap();
    let mut leaf = String::new();
    b.for_each_text_chunk(|s| leaf.push_str(s));
    assert_eq!(leaf, b.text_content());
}

#[test]
fn with_attribute_matches_get_attribute() {
    const HTML: &str = r#"<body><a href="/x" data-k="v" empty="">t</a></body>"#;
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();
    let a = dom.root().select_css("a").unwrap().next().unwrap();

    // The borrowing hook agrees with the allocating accessor on every resolution path.
    for name in [
        "href",
        "HREF",
        "data-k",
        "DATA-K",
        "empty",
        "title",
        "data-missing",
    ] {
        assert_eq!(
            a.with_attribute(name, |v| v.map(str::to_string)),
            a.get_attribute(name),
        );
    }
    // Present-but-empty is Some(""), absent is None — the distinction scan_table relies on.
    assert_eq!(
        a.with_attribute("empty", |v| v.map(str::to_string))
            .as_deref(),
        Some("")
    );
    assert_eq!(a.with_attribute("title", |v| v.map(str::to_string)), None);
}

#[test]
fn css_path_walks_from_top() {
    const HTML: &str =
        r#"<body><main><article id="a" class="x y"><p>t</p></article></main></body>"#;
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();

    let p = dom.root().select_css("p").unwrap().next().unwrap();
    assert_eq!(p.css_path(), "body > main > article#a.x.y > p");
    assert_eq!(dom.root().css_path(), "");
}

#[test]
fn element_to_html_renders_subtree() {
    const HTML: &str =
        r#"<body><main><article class="a">One<b>two</b></article><p>tail</p></main></body>"#;
    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom();

    let article = dom.root().select_css("article").unwrap().next().unwrap();
    assert_eq!(
        article.to_html(HtmlFormat::Raw),
        r#"<article class="a">One<b>two</b></article>"#
    );

    // On the root, the element render and the document render agree.
    assert_eq!(
        dom.root().to_html(HtmlFormat::Raw),
        dom.to_html(HtmlFormat::Raw)
    );
}

#[test]
fn element_navigation() {
    // body
    //  header
    //    h1
    //  main
    //   article
    //   p
    //  footer
    const HTML: &str = r#"<body><header><h1>Header</h1></header><main><article>Article</article><p>Paragraph</p></main><footer>Footer</footer></body>"#;

    let dom = crate::html::HtmlDoc::parse(HTML).unwrap().dom;
    let body = dom.root().first_child().unwrap();

    assert_eq!(body.tag(), HtmlTag::body);

    let header = body.first_child().unwrap();
    let main = header.next_sibling().unwrap();

    assert_eq!(main.tag(), HtmlTag::main);

    let p = main.last_child_all().unwrap();

    assert_eq!(p.tag(), HtmlTag::p);

    let article = p.prev_sibling().unwrap();

    assert_eq!(article.tag(), HtmlTag::article);

    let parent = article.parent().unwrap();

    assert_eq!(parent.tag(), HtmlTag::main);
}
