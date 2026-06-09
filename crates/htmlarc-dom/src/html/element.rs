use crate::{
    accessors::{
        Attributes, AttributesMut, Classes, ClassesMut, DataAttributes, DataAttributesMut,
    },
    css::{self, AttributeSelector, ParseError, Selector, SelectorList},
    dom::{DomRead, DomRef, DomRefCell, DomView, NodeIndex, Nodes, NodesView},
    error::ElementError,
    fmt::HtmlFormat,
    html::HtmlTag,
    iters::{DomIterator, ElementIter, MatchIter, RelativeIter, RevElementIter},
    stores::Class,
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
        self.dom.with_view(|view| f(view.nodes))
    }

    fn with_nodes_new<F: Fn(NodesView) -> Option<NodeIndex>>(&self, f: F) -> Option<Self> {
        self.dom
            .with_view(|view| f(view.nodes).map(|index| self.new_with_index(index)))
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

    pub fn tag(&self) -> HtmlTag {
        self.with_nodes(|nodes| nodes.tag(self.index()))
    }

    /// Returns the text value if the element is a text or comment node
    pub fn text(&self) -> Option<String> {
        self.dom
            .with_view(|view| view.text(self.index()).map(|s| s.to_string()))
    }

    /// Gathers the text content of the element and its descendants
    pub fn text_content(&self) -> String {
        self.descendants().text_chars().collect()
    }

    pub fn count_parents(&self) -> u16 {
        self.ancestors().count() as u16
    }

    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        self.dom.to_html(fmt)
    }

    pub fn css_path(&self) -> String {
        let segs: Vec<String> = Vec::with_capacity(12);

        segs.join(" > ")
    }

    pub fn writeable<'a>(&self, dom: &'a DomRefCell) -> HtmlElement<'a, DomRefCell> {
        HtmlElement::new(dom, self.index())
    }

    pub fn descendants(&self) -> ElementIter<'dom, Dom> {
        ElementIter::descendants(self)
    }

    pub fn forwards(&self) -> ElementIter<'dom, Dom> {
        ElementIter::forwards(self)
    }

    pub fn reverse(&self) -> RevElementIter<'dom, Dom> {
        RevElementIter::reverse(self)
    }

    pub fn has_attributes(&self, attrs: &[AttributeSelector]) -> bool {
        self.with_view(|view| view.has_attributes(self.index(), attrs))
    }

    pub fn attribute(&self, attr: HtmlAttr) -> Option<String> {
        self.find_attribute(attr, |v| v.map(|s| s.to_string()))
    }

    pub fn find_attribute<R, F: Fn(Option<&str>) -> R>(&self, tag: HtmlAttr, f: F) -> R {
        self.with_view(|view| {
            let val = view
                .nodes
                .attr_list_index(self.index())
                .and_then(|idx| view.attrs.list_at(idx).find(|a| a.tag == tag))
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

    pub fn has_data_attributes(&self, attrs: &[AttributeSelector]) -> bool {
        self.with_view(|view| view.has_data_attributes(self.index(), attrs))
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

    pub fn select(
        &self,
        selector: SelectorList<'dom>,
    ) -> MatchIter<'dom, Dom, ElementIter<'dom, Dom>> {
        MatchIter::new(self.forwards(), selector)
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
    ) -> Result<MatchIter<'dom, Dom, ElementIter<'dom, Dom>>, ParseError> {
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
            .find(|attr| attr.tag == HtmlAttr::id)
            .map(|attr| attr.val)
    }

    pub fn tag_id_class(&self) -> String {
        let mut tag_id_class = self.tag().to_string();
        if let Some(id) = self.id() {
            tag_id_class.push_str(&format!("#{}", id));
        }
        for class in self.classes() {
            tag_id_class.push_str(&format!(".{}", class));
        }
        tag_id_class
    }

    pub fn attributes(&self) -> Attributes<'dom, Dom> {
        Attributes {
            dom: self.dom,
            index: self.with_nodes(|nodes| nodes.attr_list_index(self.index())),
        }
    }

    pub fn data_attributes(&self) -> DataAttributes<'dom, Dom> {
        DataAttributes {
            dom: self.dom,
            index: self.with_nodes(|nodes| nodes.data_attr_list_index(self.index())),
        }
    }

    /// Obtain a read lock for the classes.
    pub fn classes(&self) -> Classes<'dom, Dom> {
        let index = self.with_nodes(|nodes| nodes.class_list_index(self.index));
        Classes {
            dom: self.dom,
            index,
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
    pub fn data_attributes_mut(&self) -> DataAttributesMut<'dom> {
        let index = self.with_nodes(|nodes| nodes.data_attr_list_index(self.index));
        DataAttributesMut {
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
