use colorful::Colorful;
use htmlarc_dom::{
    css::Selector,
    prelude::{DomRef, HtmlElement, Tag, TagIter},
};
use pretty::{Doc, RcDoc};
use smallvec::{SmallVec, ToSmallVec};
use std::ops::AddAssign;
use crate::superscript::NumStrings;

use super::{ElementString, ProbeExpression};

#[derive(Debug, Clone)]
pub struct NodeCount<'dom> {
    parent_index: Option<usize>,
    node: ElementString<'dom>,
    count: usize,
    words: SmallVec<[&'dom str; 4]>,
}

impl<'dom> NodeCount<'dom> {
    fn new(parent_index: Option<usize>, node: ElementString<'dom>, words: &[&'dom str]) -> Self {
        let words = words.to_smallvec();

        Self {
            parent_index,
            node,
            count: 1,
            words,
        }
    }
    fn increase(&mut self, count: usize, words: &[&'dom str]) {
        self.count += count;
        for word in words {
            if !self.words.contains(word) {
                self.words.push(word);
            }
        }
    }
}

#[derive(Debug, Default)]
pub struct CountedNodes<'dom> {
    nodes: Vec<Vec<NodeCount<'dom>>>,
}

impl AddAssign for CountedNodes<'_> {
    fn add_assign(&mut self, rhs: Self) {
        if self.nodes.is_empty() {
            *self = rhs;
            return;
        }
        let indexes = if let Some(nodes) = rhs.nodes.first() {
            nodes
        } else {
            return;
        };

        for (i, _) in indexes.iter().enumerate() {
            self.add_node_and_children(None, i, &rhs, 0);
        }
    }
}

impl<'dom> CountedNodes<'dom> {
    pub fn insert(
        &mut self,
        node: ElementString<'dom>,
        word: &'dom str,
        depth: usize,
        parent_index: Option<usize>,
    ) -> usize {
        if let Some(nodes) = self.nodes.get(depth) {
            if let Some(position) = nodes.iter().position(|current| {
                if let (Some(inserted_parent_index), Some(current_parent_index)) =
                    (parent_index, current.parent_index)
                {
                    self.nodes[depth - 1][inserted_parent_index].node
                        == self.nodes[depth - 1][current_parent_index].node
                        && current.node == node
                } else {
                    current.node == node
                }
            }) {
                self.nodes.get_mut(depth).unwrap()[position].increase(1, &[word]);
                position
            } else {
                let nodes = self.nodes.get_mut(depth).unwrap();
                nodes.push(NodeCount::new(parent_index, node, &[word]));
                nodes.len() - 1
            }
        } else {
            let nodes = vec![NodeCount::new(parent_index, node, &[word])];
            self.nodes.push(nodes);

            0
        }
    }

    pub fn analyze_html<Dom: DomRef>(
        &mut self,
        word: &'dom str,
        root: &HtmlElement<'dom, Dom>,
        expressions: &[ProbeExpression<'dom>],
    ) {
        let mut index_stack: Vec<(usize, u16)> = Vec::new();

        let mut iter = TagIter::new(root.dom());

        while let Some(tag) = iter.next(root.dom()) {
            match tag {
                Tag::Open(index) => {
                    let element = HtmlElement::new(root.dom(), index);
                    for expression in expressions {
                        if expression.selector.matches(&element) {
                            let node = expression.format.format(&element);

                            let parent_index = index_stack.last().map(|(i, _)| i).copied();

                            let depth = index_stack.len();

                            let node_index = self.insert(node, word, depth, parent_index);

                            index_stack.push((node_index, element.index()));
                            break;
                        }
                    }
                }
                Tag::Close(index) => {
                    if let Some((_, element_index)) = index_stack.last()
                        && *element_index == index
                    {
                        index_stack.pop();
                    }
                }
            }
        }
    }

    pub fn to_pretty_string(&self) -> String {
        const WIDTH: usize = 10;
        let mut w = Vec::new();
        self.to_doc().render(WIDTH, &mut w).unwrap();
        String::from_utf8(w).unwrap()
    }

    fn to_doc(&self) -> RcDoc<'_, ()> {
        fn children_doc<'a>(
            node_index: usize,
            nodes: &'a [Vec<NodeCount>],
            depth: usize,
        ) -> RcDoc<'a, ()> {
            const TAB: &str = "  ";
            const COLORED: bool = false;

            let node = &nodes[depth][node_index];

            let tab = TAB.repeat(depth);

            let doc = if node.node.with_words {
                RcDoc::text(format!(
                    "{}{}[{}]",
                    tab,
                    num_name(depth, &node.node.to_string(), node.count as u32, COLORED),
                    node.words.join(",")
                ))
            } else {
                RcDoc::text(format!(
                    "{}{}",
                    tab,
                    num_name(depth, &node.node.to_string(), node.count as u32, COLORED)
                ))
            };

            if let Some(deeper_nodes) = nodes.get(depth + 1) {
                doc.append(Doc::line_())
                    .append(RcDoc::intersperse(
                        deeper_nodes
                            .iter()
                            .enumerate()
                            .filter(|(_, n)| n.parent_index == Some(node_index))
                            .map(|(i, _)| {
                                RcDoc::text("").append(children_doc(i, nodes, depth + 1))
                            }),
                        Doc::nil(),
                    ))
                    .group()
            } else {
                doc.append(Doc::line_())
            }
        }

        if let Some(nodes) = self.nodes.first() {
            RcDoc::text("").append(RcDoc::intersperse(
                nodes
                    .iter()
                    .enumerate()
                    .map(|(i, _)| RcDoc::nil().append(children_doc(i, &self.nodes, 0))),
                Doc::nil(),
            ))
        } else {
            RcDoc::nil()
        }
    }

    fn add_node_and_children(
        &mut self,
        lhs_parent_index: Option<usize>,
        rhs_node_index: usize,
        rhs: &CountedNodes<'dom>,
        depth: usize,
    ) {
        let rhs_node = &rhs.nodes[depth][rhs_node_index];

        let added_node_index = if let Some(nodes) = self.nodes.get_mut(depth) {
            if let Some((index, lhs_node)) = nodes
                .iter_mut()
                .enumerate()
                .find(|(_, n)| n.node == rhs_node.node)
            {
                lhs_node.increase(rhs_node.count, &rhs_node.words);
                index
            } else {
                self.nodes[depth].push(NodeCount::new(
                    lhs_parent_index,
                    rhs_node.node.clone(),
                    &rhs_node.words,
                ));
                self.nodes[depth].len() - 1
            }
        } else {
            self.nodes.push(vec![NodeCount::new(
                None,
                rhs_node.node.clone(),
                &rhs_node.words,
            )]);

            self.nodes[depth].len() - 1
        };

        let deeper_depth = depth + 1;
        if let Some(deeper_nodes) = rhs.nodes.get(deeper_depth) {
            for (i, _) in deeper_nodes
                .iter()
                .enumerate()
                .filter(|(_, n)| n.parent_index == Some(rhs_node_index))
            {
                self.add_node_and_children(Some(added_node_index), i, rhs, deeper_depth);
            }
        }
    }
}

pub fn num_name(lvl: usize, name: &str, num: u32, colored: bool) -> String {
    if colored {
        format!(
            "{}{name}{}",
            lvl.to_superscript_chars().dim(),
            num.to_superscript().green()
        )
    } else {
        format!(
            "{}{name}{}",
            lvl.to_superscript_chars(),
            num.to_superscript()
        )
    }
}

/// section
///   header
///     h1#header
///   summary
///   footer
///     div
///       a
///       p
///         h1#footer
/// aside
#[cfg(test)]
const HTML1: &str = r#"<section><header><h1 id="h1-header"></h1></header><summary></summary><footer><div><a></a><p><h1 id="h1-footer"></h1></p></div></footer></section><aside></aside>"#;

/// section
///   div
///     header
///       h1#header
///   footer
///     div
///       a
///       p
///       h1#footer
/// aside
#[cfg(test)]
const HTML2: &str = r#"<section><div><header><h1 id="h1-header"></h1></header></div><footer><div><a></a><p></p><h1 id="h1-footer"></h1></div></footer></section><aside></aside>"#;

#[cfg(test)]
fn probe_expressions<'a>() -> Vec<ProbeExpression<'a>> {
    let expr_1 = "section h1, div, p => HtmlFmt[id]";
    let probe_1 = ProbeExpression::try_from(expr_1).unwrap();

    let expr_2 = "section, a, aside => HtmlFmt[id]@words";
    let probe_2 = ProbeExpression::try_from(expr_2).unwrap();

    vec![probe_1, probe_2]
}

#[test]
fn nodes_insertion() {
    let doc = htmlarc_dom::prelude::HtmlDoc::parse(HTML1).unwrap();
    let dom = doc.dom();
    let root = htmlarc_dom::prelude::DomRead::root(&dom);
    let expressions = probe_expressions();

    let mut nodes = CountedNodes::default();

    nodes.analyze_html("test", &root, &expressions);

    insta::assert_snapshot!(nodes.to_pretty_string());
}

#[test]
fn nodes_addition() {
    let expressions = probe_expressions();

    let doc1 = htmlarc_dom::prelude::HtmlDoc::parse(HTML1).unwrap();
    let dom1 = doc1.dom();
    let root1 = htmlarc_dom::prelude::DomRead::root(&dom1);
    let mut nodes_1 = CountedNodes::default();
    nodes_1.analyze_html("foo", &root1, &expressions);

    let doc2 = htmlarc_dom::prelude::HtmlDoc::parse(HTML2).unwrap();
    let dom2 = Box::leak(Box::new(doc2.dom()));
    let root2 = htmlarc_dom::prelude::DomRead::root(dom2);
    let mut nodes_2 = CountedNodes::default();
    nodes_2.analyze_html("bar", &root2, &expressions);

    nodes_1 += nodes_2;

    insta::assert_snapshot!(nodes_1.to_pretty_string());

    let mut nodes_3 = CountedNodes::default();
    nodes_3.analyze_html("foo", &root1, &expressions);
    nodes_3.analyze_html("bar", &root2, &expressions);

    assert_eq!(nodes_1.to_pretty_string(), nodes_3.to_pretty_string());

    let mut nodes_4 = CountedNodes::default();
    nodes_4 += nodes_1;

    assert_eq!(nodes_3.to_pretty_string(), nodes_4.to_pretty_string());
}
