use std::{
    fmt::{self, Debug},
    ops::Range,
};

use rkyv::{Archive, Deserialize, Serialize};

use crate::html::HtmlTag;
use crate::stores::ListIndex;

/// An html node consists of bytes:
/// 0  HtmlTag
/// 1-2 parent index
/// 3-4 prev sibling index
/// 5-6 next sibling index
/// 7-8 first child index
/// 9-10 last child index
/// 11-12 class list index
/// 13-14 attr list index
/// 15-16 data attr list index
///
/// A text or comment node consists of bytes:
/// 0   255: text, 254: comment
/// 1-2 parent index
/// 3-4 prev sibling index
/// 5-6 next sibling index
/// 7-10 text start
/// 11-14 text end
/// 15-16 not used
const PARENT_OFFSET: usize = 1;
const PREV_SIBLING_OFFSET: usize = 3;
const NEXT_SIBLING_OFFSET: usize = 5;
const FIRST_CHILD_OFFSET: usize = 7;
const LAST_CHILD_OFFSET: usize = 9;
const CLASS_LIST_OFFSET: usize = 11;
const ATTR_LIST_OFFSET: usize = 13;
const DATA_ATTR_OFFSET: usize = 15;
const TEXT_START_OFFSET: usize = 7;
const TEXT_END_OFFSET: usize = 11;
const NODE_SIZE: usize = 17;

/// Encapsulates a byte vector, which is manipulated with index,
/// where each index represents a node in the tree.
#[derive(Archive, Deserialize, Serialize, PartialEq, Hash, Clone)]
pub(crate) struct Nodes {
    bytes: Vec<u8>,
}

impl Default for Nodes {
    fn default() -> Self {
        Self::new()
    }
}

impl Nodes {
    /// create a new nodevec with a root node
    pub fn new() -> Self {
        let mut me = Self { bytes: Vec::new() };
        me.add_node(HtmlTag::sys_root, None, None, None);
        me
    }

    pub(crate) fn new_based_on(nodes: &Nodes) -> Self {
        let bytes = Vec::with_capacity(nodes.bytes.len());
        Self { bytes }
    }

    pub fn len(&self) -> usize {
        self.bytes.len() / NODE_SIZE
    }

    pub fn is_string_node(&self, index: u16) -> bool {
        let tag = self.tag(index);
        tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment
    }

    pub fn is_inline_element(&self, index: u16) -> bool {
        self.tag(index).is_inline_element()
    }

    pub fn is_block_element(&self, index: u16) -> bool {
        self.tag(index).is_block_element()
    }

    pub fn tag(&self, index: u16) -> HtmlTag {
        HtmlTag::from_repr(self.bytes[index as usize * NODE_SIZE]).unwrap()
    }

    pub fn parent_index(&self, index: u16) -> Option<u16> {
        self.opt_u16_at(index, PARENT_OFFSET)
    }

    pub fn prev_sibling_index(&self, index: u16) -> Option<u16> {
        self.opt_u16_at(index, PREV_SIBLING_OFFSET)
    }

    pub fn next_sibling_index(&self, index: u16) -> Option<u16> {
        self.opt_u16_at(index, NEXT_SIBLING_OFFSET)
    }

    pub fn first_child_index(&self, index: u16) -> Option<u16> {
        match self.is_string_node(index) {
            true => None,
            false => self.opt_u16_at(index, FIRST_CHILD_OFFSET),
        }
    }

    pub fn last_child_index(&self, index: u16) -> Option<u16> {
        match self.is_string_node(index) {
            true => None,
            false => self.opt_u16_at(index, LAST_CHILD_OFFSET),
        }
    }

    pub fn text_range(&self, index: u16) -> Range<u32> {
        debug_assert!(self.is_string_node(index));
        let start = self.u32_at(index, TEXT_START_OFFSET);
        let end = self.u32_at(index, TEXT_END_OFFSET);
        start..end
    }

    pub fn class_list_index(&self, index: u16) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            self.opt_u16_at(index, CLASS_LIST_OFFSET)
                .map(ListIndex::from)
        }
    }

    pub fn attr_list_index(&self, index: u16) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            self.opt_u16_at(index, ATTR_LIST_OFFSET)
                .map(ListIndex::from)
        }
    }

    pub fn data_attr_list_index(&self, index: u16) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            self.opt_u16_at(index, DATA_ATTR_OFFSET)
                .map(ListIndex::from)
        }
    }

    pub fn add_as_next_sibling(&mut self, index: u16, tag: HtmlTag) -> u16 {
        let parent_index = self.parent_index(index);
        let prev_sibling = Some(index);
        let next_sibling = self.next_sibling_index(index);
        let parent_last_child = self.last_child_index(parent_index.unwrap());

        // add new node as next sibling of the current node
        let new_next_sibling = self.add_node(tag, parent_index, prev_sibling, next_sibling);

        // the current node now has the new node as next sibling
        self.set_next_sibling_index(index, Some(new_next_sibling));

        // the former next sibling now has the new node as previous sibling
        if let Some(sibling) = next_sibling {
            self.set_prev_sibling_index(sibling, Some(new_next_sibling));
        }

        // if the current node is the last child of the parent, the new node is now the last child
        if Some(index) == parent_last_child {
            self.set_last_child_index(parent_index.unwrap(), Some(new_next_sibling));
        }
        new_next_sibling
    }

    pub fn add_as_prev_sibling(&mut self, index: u16, tag: HtmlTag) -> u16 {
        let parent_index = self.parent_index(index);
        let prev_sibling = self.prev_sibling_index(index);
        let next_sibling = Some(index);
        let parent_first_child = self.first_child_index(parent_index.unwrap());

        // add new node as previous sibling of the current node
        let new_prev_node = self.add_node(tag, parent_index, prev_sibling, next_sibling);

        // the current node now has the new node as previous sibling
        self.set_prev_sibling_index(index, Some(new_prev_node));

        // the former previous sibling now has the new node as next sibling
        if let Some(sibling) = prev_sibling {
            self.set_next_sibling_index(sibling, Some(new_prev_node));
        }

        // if the current node is the first child of the parent, the new node is now the first child
        if Some(index) == parent_first_child {
            self.set_first_child_index(parent_index.unwrap(), Some(new_prev_node));
        }
        new_prev_node
    }

    pub fn add_as_first_child(&mut self, index: u16, tag: HtmlTag) -> u16 {
        let parent_index = Some(index);
        let next_sibling = self.first_child_index(index);

        let new_index = self.add_node(tag, parent_index, None, next_sibling);

        // the former first child now has the new node as previous sibling
        if let Some(child) = next_sibling {
            self.set_prev_sibling_index(child, Some(new_index));
        }

        // if the current node has no children, the new node is also the last child
        if next_sibling.is_none() {
            self.set_last_child_index(index, Some(new_index));
        }

        // the current node now has the new node as first child
        self.set_first_child_index(index, Some(new_index));
        new_index
    }

    pub fn add_as_last_child(&mut self, index: u16, tag: HtmlTag) -> u16 {
        // the added node parent is the current node and the previous sibling is the current last child
        let parent_index = Some(index);
        let prev_sibling = self.last_child_index(index);

        let new_index = self.add_node(tag, parent_index, prev_sibling, None);

        // the former last child now has the new node as next sibling
        if let Some(child) = prev_sibling {
            self.set_next_sibling_index(child, Some(new_index));
        } else {
            // if the current node has no children, the new node is also the first child
            self.set_first_child_index(index, Some(new_index));
        }

        // the current node now has the new node as last child
        self.set_last_child_index(index, Some(new_index));
        new_index
    }

    fn join(&mut self, prev_sibling: u16, next_sibling: u16) {
        self.set_prev_sibling_index(next_sibling, Some(prev_sibling));
        self.set_next_sibling_index(prev_sibling, Some(next_sibling));
    }

    fn update_children_parent(&mut self, first_child: u16, parent: u16) {
        let mut current = first_child;
        loop {
            self.set_parent_index(current, Some(parent));

            if let Some(sibling) = self.next_sibling_index(current) {
                current = sibling;
            } else {
                break;
            }
        }
    }
    pub fn remove_children(&mut self, index: u16) {
        self.set_first_child_index(index, None);
        self.set_last_child_index(index, None);
    }

    /// Remove a node by removing all references to it and removing
    /// it's parent reference. The node itself keeps the current
    /// sibling references which is useful when iterating
    pub fn remove(&mut self, index: u16) -> Option<u16> {
        // node A : previous sibling
        // node B : current node
        // node C : next sibling

        let prev_sibling = self.prev_sibling_index(index);
        let next_sibling = self.next_sibling_index(index);
        let parent_index = self.parent_index(index).unwrap();
        let parent_first_child = self.first_child_index(parent_index);
        let parent_last_child = self.last_child_index(parent_index);

        // remove the current node's reference to its parent
        self.set_parent_index(index, None);

        // if the current node is the first child of the parent, the next sibling is now the first child
        if Some(index) == parent_first_child {
            self.set_first_child_index(parent_index, next_sibling);
        }

        // if the current node is the last child of the parent, the previous sibling is now the last child
        if Some(index) == parent_last_child {
            self.set_last_child_index(parent_index, prev_sibling);
        }

        let mut new_index = None;
        if let Some(prev) = prev_sibling {
            if let Some(next) = next_sibling {
                // node A now has node C as next sibling
                self.set_next_sibling_index(prev, Some(next));

                // node C now has node A as previous sibling
                self.set_prev_sibling_index(next, Some(prev));
            } else {
                // node A now has no next sibling
                self.set_next_sibling_index(prev, None);
                // node A is now the last child of the parent
                self.set_last_child_index(parent_index, Some(prev));
            }
            // move the cursor to the previous index
            new_index = Some(prev);
        } else if let Some(next) = next_sibling {
            // node C now has no previous sibling
            self.set_prev_sibling_index(next, None);
            // node C is now the first child of the parent
            self.set_first_child_index(parent_index, Some(next));
            // move the cursor to the next index
            new_index = Some(next);
        }

        if prev_sibling.is_none() && next_sibling.is_none() {
            new_index = Some(parent_index);
        }
        new_index
    }

    pub fn unwrap_node(&mut self, index: u16) -> Option<u16> {
        let prev_sibling = self.prev_sibling_index(index);
        let next_sibling = self.next_sibling_index(index);
        let parent_index = self.parent_index(index).unwrap();
        let first_child = self.first_child_index(index);
        let last_child = self.last_child_index(index);

        // the unwraped node shouldn't reference any other node
        self.set_prev_sibling_index(index, None);
        self.set_next_sibling_index(index, None);
        self.set_first_child_index(index, None);
        self.set_last_child_index(index, None);

        // remove the current node's reference to its parent
        self.set_parent_index(index, None);

        match (prev_sibling, first_child, last_child, next_sibling) {
            (Some(prev), None, None, None) => {
                // prev becomes the last index of the parent
                self.set_last_child_index(parent_index, Some(prev));
                // the prev node now has no next sibling
                self.set_next_sibling_index(prev, None);
            }
            (None, None, None, Some(next)) => {
                // next becomes the first child of the parent
                self.set_first_child_index(parent_index, Some(next));
                // the next node now has no previous sibling
                self.set_prev_sibling_index(next, None);
            }
            (Some(prev), None, None, Some(next)) => {
                // link prev and next
                self.join(prev, next);
            }
            (None, Some(first), Some(last), None) => {
                // first becomes the first child of the parent
                self.set_first_child_index(parent_index, Some(first));

                // last becomes the last child of the parent
                self.set_last_child_index(parent_index, Some(last));

                // all nodes from first to last has a new parent
                self.update_children_parent(first, parent_index);
            }
            (None, Some(first), Some(last), Some(next)) => {
                // first becomes the first child of the parent
                self.set_first_child_index(parent_index, Some(first));

                // link last and next
                self.join(last, next);

                // all nodes from first to last has a new parent
                self.update_children_parent(first, parent_index);
            }
            (Some(prev), Some(first), Some(last), None) => {
                // link prev and first
                self.join(prev, first);

                // last becomes the last child of the parent
                self.set_last_child_index(parent_index, Some(last));

                // all nodes from first to last has a new parent
                self.update_children_parent(first, parent_index);
            }
            (Some(prev), Some(first), Some(last), Some(next)) => {
                // link prev and first
                self.join(prev, first);

                // link last and next
                self.join(last, next);

                // all nodes from first to last has a new parent
                self.update_children_parent(first, parent_index);
            }
            (None, None, None, None) => {
                // we are unwraping an empty element that is an only child
                // we remove the parent's reference to the unwraped node
                self.set_first_child_index(parent_index, None);
                self.set_last_child_index(parent_index, None);
            }
            _ => {}
        }

        first_child
    }
    /// Replaces the current node with another one from the tree,
    pub fn replace_with(&mut self, index: u16, new_index: u16) {
        // node A : previous sibling
        // node B : current node
        // node C : next sibling
        // node X : replacement node

        let substitute_parent = self.parent_index(new_index);
        let substitute_prev_sibling = self.prev_sibling_index(new_index);
        let substitute_next_sibling = self.next_sibling_index(new_index);

        // update node X's siblings and parent references
        match (substitute_prev_sibling, substitute_next_sibling) {
            (Some(prev), Some(next)) => {
                self.set_next_sibling_index(prev, Some(next));
                self.set_prev_sibling_index(next, Some(prev))
            }
            (Some(prev), None) => {
                self.set_next_sibling_index(prev, None);
                self.set_last_child_index(substitute_parent.unwrap(), Some(prev));
            }
            (None, Some(next)) => {
                self.set_prev_sibling_index(next, None);
                self.set_first_child_index(substitute_parent.unwrap(), Some(next));
            }
            (None, None) => {
                self.set_first_child_index(substitute_parent.unwrap(), None);
                self.set_last_child_index(substitute_parent.unwrap(), None);
            }
        }

        let parent = self.parent_index(index);
        let prev_sibling = self.prev_sibling_index(index);
        let next_sibling = self.next_sibling_index(index);

        // update node X's new siblings references
        match (prev_sibling, next_sibling) {
            (Some(prev), Some(next)) => {
                self.set_next_sibling_index(prev, Some(new_index));
                self.set_prev_sibling_index(new_index, prev_sibling);
                self.set_next_sibling_index(new_index, next_sibling);
                self.set_prev_sibling_index(next, Some(new_index));
            }
            (Some(prev), None) => {
                self.set_next_sibling_index(prev, Some(new_index));
                self.set_prev_sibling_index(new_index, prev_sibling);
                self.set_next_sibling_index(new_index, None);
                self.set_last_child_index(parent.unwrap(), Some(new_index));
            }
            (None, Some(next)) => {
                self.set_first_child_index(parent.unwrap(), Some(new_index));
                self.set_prev_sibling_index(new_index, None);
                self.set_next_sibling_index(new_index, next_sibling);
                self.set_prev_sibling_index(next, Some(new_index));
            }
            (None, None) => {
                self.set_first_child_index(parent.unwrap(), Some(new_index));
                self.set_prev_sibling_index(new_index, None);
                self.set_next_sibling_index(new_index, None);
                self.set_last_child_index(parent.unwrap(), Some(new_index));
            }
        }

        // node X now has node B's parent as its parent
        self.set_parent_index(new_index, parent);

        // node B now has no parent
        self.set_parent_index(index, None);

        // node B now has no previous sibling
        self.set_prev_sibling_index(index, None);

        // node B now has no next sibling
        self.set_next_sibling_index(index, None);
    }

    fn u32_at(&self, index: u16, offset: usize) -> u32 {
        let pos = index as usize * NODE_SIZE + offset;
        u32::from_le_bytes([
            self.bytes[pos],
            self.bytes[pos + 1],
            self.bytes[pos + 2],
            self.bytes[pos + 3],
        ])
    }

    fn set_u32_at(&mut self, index: u16, offset: usize, value: u32) {
        let pos = index as usize * NODE_SIZE + offset;
        let bytes = value.to_le_bytes();
        self.bytes[pos] = bytes[0];
        self.bytes[pos + 1] = bytes[1];
        self.bytes[pos + 2] = bytes[2];
        self.bytes[pos + 3] = bytes[3];
    }

    fn opt_u16_at(&self, index: u16, offset: usize) -> Option<u16> {
        let pos = index as usize * NODE_SIZE + offset;
        opt_u16([self.bytes[pos], self.bytes[pos + 1]])
    }

    fn set_opt_u16_at(&mut self, index: u16, offset: usize, value: Option<u16>) {
        let pos = index as usize * NODE_SIZE + offset;
        if let Some(val) = value {
            let bytes = val.to_le_bytes();
            self.bytes[pos] = bytes[0];
            self.bytes[pos + 1] = bytes[1];
        } else {
            self.bytes[pos] = u8::MAX;
            self.bytes[pos + 1] = u8::MAX;
        }
    }

    pub(crate) fn set_parent_index(&mut self, index: u16, value: Option<u16>) {
        self.set_opt_u16_at(index, PARENT_OFFSET, value);
    }

    pub(crate) fn set_prev_sibling_index(&mut self, index: u16, value: Option<u16>) {
        self.set_opt_u16_at(index, PREV_SIBLING_OFFSET, value);
    }

    pub(crate) fn set_next_sibling_index(&mut self, index: u16, value: Option<u16>) {
        self.set_opt_u16_at(index, NEXT_SIBLING_OFFSET, value);
    }

    pub(crate) fn set_first_child_index(&mut self, index: u16, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        self.set_opt_u16_at(index, FIRST_CHILD_OFFSET, value);
    }

    pub(crate) fn set_last_child_index(&mut self, index: u16, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        self.set_opt_u16_at(index, LAST_CHILD_OFFSET, value);
    }

    pub(crate) fn set_class_list_index(&mut self, index: u16, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        self.set_opt_u16_at(index, CLASS_LIST_OFFSET, value);
    }

    pub(crate) fn set_attr_list_index(&mut self, index: u16, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        self.set_opt_u16_at(index, ATTR_LIST_OFFSET, value);
    }

    pub(crate) fn set_data_attr_list_index(&mut self, index: u16, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        self.set_opt_u16_at(index, DATA_ATTR_OFFSET, value);
    }

    pub(crate) fn set_text_range(&mut self, index: u16, range: Range<u32>) {
        debug_assert!(self.is_string_node(index));
        self.set_u32_at(index, TEXT_START_OFFSET, range.start);
        self.set_u32_at(index, TEXT_END_OFFSET, range.end);
    }

    fn push_opt_u16(&mut self, value: Option<u16>) {
        if let Some(val) = value {
            self.bytes.push(val as u8);
            self.bytes.push((val >> 8) as u8);
        } else {
            self.bytes.push(u8::MAX);
            self.bytes.push(u8::MAX);
        }
    }

    pub(crate) fn add_node(
        &mut self,
        tag: HtmlTag,
        parent_index: Option<u16>,
        prev_sibling: Option<u16>,
        next_sibling: Option<u16>,
    ) -> u16 {
        let index = (self.bytes.len() / NODE_SIZE) as u16;
        self.bytes.push(tag as u8); // 0
        self.push_opt_u16(parent_index); // 1-2
        self.push_opt_u16(prev_sibling); // 3-4
        self.push_opt_u16(next_sibling); // 5-6
        self.push_opt_u16(None); // 7-8
        self.push_opt_u16(None); // 9-10
        if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
            self.bytes.resize(self.bytes.len() + 4, 0);
        } else {
            self.push_opt_u16(None); // 11-12
            self.push_opt_u16(None); // 13-14
        }
        self.push_opt_u16(None); // 15-16
        index
    }

    pub(crate) fn dbg_table_string(&self, index: u16) -> String {
        format!(
            "[{:2}] {ps}  {ns}  {fc}  {lc}   {level}{tag:?}",
            index,
            level = self.parent_list(index).join(""),
            tag = self.tag(index),
            ps = dbg_w(self.prev_sibling_index(index), 8),
            ns = dbg_w(self.next_sibling_index(index), 8),
            fc = dbg_w(self.first_child_index(index), 8),
            lc = dbg_w(self.last_child_index(index), 7),
        )
    }

    fn parent_list(&self, mut index: u16) -> Vec<String> {
        let mut list = Vec::new();
        while let Some(parent) = self.parent_index(index) {
            list.push(format!("{parent:<2} > "));
            index = parent;
        }
        list.reverse();
        list
    }
}

impl Debug for Nodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag"
        )?;
        for i in 0..self.len() {
            writeln!(f, "{}", self.dbg_table_string(i as u16))?;
        }
        Ok(())
    }
}

fn dbg_w(val: Option<u16>, width: usize) -> String {
    val.map(|v| format!("{v:width$}"))
        .unwrap_or_else(|| " ".repeat(width))
}

#[inline]
fn opt_u16(bytes: [u8; 2]) -> Option<u16> {
    let num = u16::from_le_bytes(bytes);
    if num == u16::MAX { None } else { Some(num) }
}

#[test]
fn test_opt_u16() {
    assert_eq!(opt_u16([0, 0]), Some(0));
    assert_eq!(opt_u16([255, 255]), None);
}

#[test]
fn test_single_node_empy() {
    let mut vec = Nodes::new();
    // check the values
    assert_eq!(vec.tag(0), HtmlTag::sys_root);
    assert_eq!(vec.parent_index(0), None);
    assert_eq!(vec.prev_sibling_index(0), None);
    assert_eq!(vec.next_sibling_index(0), None);
    assert_eq!(vec.first_child_index(0), None);
    assert_eq!(vec.last_child_index(0), None);
    assert_eq!(vec.class_list_index(0), None);
    assert_eq!(vec.attr_list_index(0), None);

    let index = vec.add_node(HtmlTag::abbr, Some(1), Some(2), Some(3));
    vec.set_first_child_index(index, Some(4));
    vec.set_last_child_index(index, Some(5));

    println!("{:?}", vec.bytes);
    assert_eq!(index, 1);
    assert_eq!(vec.tag(1), HtmlTag::abbr);
    assert_eq!(vec.parent_index(1), Some(1));
    assert_eq!(vec.prev_sibling_index(1), Some(2));
    assert_eq!(vec.next_sibling_index(1), Some(3));
    assert_eq!(vec.first_child_index(1), Some(4));
    assert_eq!(vec.last_child_index(1), Some(5));
}
