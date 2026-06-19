use tinyvec::TinyVec;

use crate::dom::{DomRead, NodeIndex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Open(NodeIndex),
    Close(NodeIndex),
}

impl Default for Tag {
    fn default() -> Self {
        Self::Open(NodeIndex::ROOT)
    }
}

pub struct TagIter {
    stack: TinyVec<[Tag; 32]>,
}

impl TagIter {
    pub fn new<Dom: DomRead>(dom: &Dom) -> Self {
        let mut stack = TinyVec::new();
        if let Some(root_child) = dom.with_nodes(|nodes| nodes.first_child_index(NodeIndex::ROOT)) {
            // We never include the root node, so we start with the first child
            // but since we need to iterate through the child siblings, which is
            // not done for the last element in the stack, we need to include
            stack.push(Tag::Close(NodeIndex::ROOT));
            stack.push(Tag::Open(root_child));
        }
        Self { stack }
    }

    pub fn next<Dom: DomRead>(&mut self, dom: &Dom) -> Option<Tag> {
        let stage = self.stack.pop()?;
        match stage {
            Tag::Open(index) => {
                self.stack.push(Tag::Close(index));
                if let Some(child_index) = dom.with_nodes(|nodes| nodes.first_child_index(index)) {
                    self.stack.push(Tag::Open(child_index));
                }
            }
            // we don't return the root node
            Tag::Close(index) if index == NodeIndex::ROOT => return None,
            // we don't iterate through the last element's siblings
            Tag::Close(_) if self.stack.is_empty() => {}
            Tag::Close(index) => {
                if let Some(sibling) = dom.with_nodes(|nodes| nodes.next_sibling_index(index)) {
                    self.stack.push(Tag::Open(sibling));
                }
            }
        }
        Some(stage)
    }
}

#[cfg(test)]
use crate::prelude::*;

#[test]
fn tag_iter_plain() {
    use crate::dom::tests::nodes_tests::add_div_and_class;

    let mut inner = DomInner::default();

    let div_a = add_div_and_class(&mut inner, NodeIndex::ROOT, "a");
    let div_aa = add_div_and_class(&mut inner, div_a, "aa");
    let div_aaa = add_div_and_class(&mut inner, div_aa, "aaa");
    let div_ab = add_div_and_class(&mut inner, div_a, "ab");
    let div_ac = add_div_and_class(&mut inner, div_a, "ac");
    let div_aca = add_div_and_class(&mut inner, div_ac, "aca");
    let div_acb = add_div_and_class(&mut inner, div_ac, "acb");

    let mut iter = TagIter::new(&inner);

    assert_eq!(iter.next(&inner), Some(Tag::Open(div_a)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_aa)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_aaa)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_aaa)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_aa)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_ab)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_ab)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_ac)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_aca)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_aca)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_acb)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_acb)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_ac)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_a)));
    assert_eq!(iter.next(&inner), None);

    let txt = HtmlFormat::Raw.to_html(inner.view(), NodeIndex::ROOT);
    insta::assert_snapshot!(txt, @r###"<div class="a"><div class="aa"><div class="aaa"></div></div><div class="ab"></div><div class="ac"><div class="aca"></div><div class="acb"></div></div></div>"###);

    let txt = HtmlFormat::Raw.to_html(inner.view(), div_aca);
    insta::assert_snapshot!(txt, @r###"<div class="aca"></div>"###);

    let txt = HtmlFormat::Raw.to_html(inner.view(), div_ac);
    insta::assert_snapshot!(txt, @r###"<div class="ac"><div class="aca"></div><div class="acb"></div></div>"###);
}

#[test]
fn tag_iter_root_two_children() {
    use crate::dom::tests::nodes_tests::add_div_and_class;

    let mut inner = DomInner::default();

    let div_a = add_div_and_class(&mut inner, NodeIndex::ROOT, "a");
    let div_b = add_div_and_class(&mut inner, NodeIndex::ROOT, "b");

    let mut iter = TagIter::new(&inner);

    assert_eq!(iter.next(&inner), Some(Tag::Open(div_a)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_a)));
    assert_eq!(iter.next(&inner), Some(Tag::Open(div_b)));
    assert_eq!(iter.next(&inner), Some(Tag::Close(div_b)));
    assert_eq!(iter.next(&inner), None);

    let txt = HtmlFormat::Raw.to_html(inner.view(), NodeIndex::ROOT);

    insta::assert_snapshot!(txt, @r###"<div class="a"></div><div class="b"></div>"###);
}

#[test]
fn tag_div() {
    use crate::html::HtmlTag;

    let mut inner = DomInner::default();

    inner.append_text_child(HtmlTag::sys_text, NodeIndex::ROOT, " ");
    let div = inner.nodes.add_as_last_child(NodeIndex::ROOT, HtmlTag::div);
    inner.append_text_child(HtmlTag::sys_text, div, "hi");
    inner.nodes.add_as_last_child(div, HtmlTag::span);

    let txt = HtmlFormat::Raw.to_html(inner.view(), NodeIndex::ROOT);

    insta::assert_snapshot!(txt, @" <div>hi<span></span></div>");
}
