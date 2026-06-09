use crate::prelude::*;
use tinyvec::TinyVec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagStage {
    Open,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementStage {
    pub index: NodeIndex,
    pub depth: u16,
    pub stage: TagStage,
}

impl ElementStage {
    pub fn new(index: NodeIndex, depth: u16, stage: TagStage) -> Self {
        Self {
            index,
            depth,
            stage,
        }
    }
}

impl Default for ElementStage {
    fn default() -> Self {
        Self::new(NodeIndex::ROOT, 0, TagStage::Open)
    }
}

fn close(index: NodeIndex, depth: u16) -> ElementStage {
    ElementStage::new(index, depth, TagStage::Close)
}

fn open(index: NodeIndex, depth: u16) -> ElementStage {
    ElementStage::new(index, depth, TagStage::Open)
}

pub struct TagIter<'a> {
    pub dom: DomView<'a>,
    stack: TinyVec<[ElementStage; 32]>,
}

impl<'a> TagIter<'a> {
    pub fn new(dom: DomView<'a>, index: NodeIndex) -> Self {
        let mut stack = TinyVec::new();
        if index == NodeIndex::ROOT {
            if let Some(root_child) = dom.nodes.first_child_index(index) {
                // We never include the root node, so we start with the first child
                // but since we need to iterate through the child siblings, which is
                // not done for the last element in the stack, we need to include
                stack.push(close(NodeIndex::ROOT, 0));
                stack.push(open(root_child, 0));
            }
        } else {
            stack.push(open(index, 0));
        }
        Self { dom, stack }
    }
}

impl Iterator for TagIter<'_> {
    type Item = ElementStage;

    fn next(&mut self) -> Option<Self::Item> {
        let info = self.stack.pop()?;
        let dom = self.dom;
        match info.stage {
            TagStage::Open => {
                self.stack.push(close(info.index, info.depth));
                if let Some(child_index) = dom.nodes.first_child_index(info.index) {
                    self.stack.push(open(child_index, info.depth + 1));
                }
                Some(open(info.index, info.depth))
            }
            TagStage::Close => {
                if info.index == NodeIndex::ROOT {
                    return None;
                } else if self.stack.is_empty() {
                    // we don't iterate through the last element's siblings
                } else if let Some(sibling) = dom.nodes.next_sibling_index(info.index) {
                    self.stack.push(open(sibling, info.depth));
                }
                Some(close(info.index, info.depth))
            }
        }
    }
}

#[cfg(test)]
use crate::{dom::DomInner, html::HtmlDoc};

#[cfg(test)]
fn tag_string(iter: TagIter<'_>, dom: &DomInner) -> String {
    use TagStage::*;
    iter.map(|el| {
        let tag = dom.dom_view().nodes.tag(el.index);
        let indent = "  ".repeat(el.depth as usize);
        let stage = match el.stage {
            Open => "+",
            Close => "-",
        };

        format!("{indent}{stage}{tag}")
    })
    .collect::<Vec<_>>()
    .join("\n")
}

#[test]
fn tag_iter_plain() {
    let html = r#"
    <div><span>hi</span><br/></div>
    <h1><i>bye</i></h1>
"#
    .trim();

    let dom = HtmlDoc::parse(html).unwrap().dom();
    let iter = TagIter::new(dom.dom_view(), NodeIndex::ROOT);

    insta::assert_snapshot!(tag_string(iter, &dom), @r###"
    +div
      +span
        +text
        -text
      -span
      +br
      -br
    -div
    +text
    -text
    +h1
      +i
        +text
        -text
      -i
    -h1
    "###);
}
