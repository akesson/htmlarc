use std::{
    fmt::{self, Debug},
    ops::Range,
};

use rkyv::{Archive, Deserialize, Serialize};

use crate::dom::NodeIndex;
use crate::html::HtmlTag;
use crate::stores::ListIndex;

/// Per-document node-index width.
///
/// Node links (parent/prev/next/first-child/last-child) are packed either 2 bytes
/// (`U16`) or 3 bytes (`U24`) wide. Documents are *always built* at [`NodeWidth::U24`]
/// (lifting the old 65,535-node ceiling to ~16.7M); at serialize time small ones are
/// down-packed to `U16` (see [`Nodes::into_optimal_width`]). Store-list indices
/// (class/attr/data) and the text-node `u32` offsets keep their own fixed widths —
/// only their *byte offsets* shift with the node width.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeWidth {
    U16,
    U24,
}

const WIDTH_U16: u8 = 0;
const WIDTH_U24: u8 = 1;

impl NodeWidth {
    const fn from_u8(v: u8) -> Self {
        if v == WIDTH_U24 {
            NodeWidth::U24
        } else {
            NodeWidth::U16
        }
    }

    const fn as_u8(self) -> u8 {
        match self {
            NodeWidth::U16 => WIDTH_U16,
            NodeWidth::U24 => WIDTH_U24,
        }
    }

    /// Bytes per node-link slot.
    const fn slot(self) -> usize {
        match self {
            NodeWidth::U16 => 2,
            NodeWidth::U24 => 3,
        }
    }

    /// The all-ones "no node" sentinel for this width.
    const fn sentinel(self) -> u32 {
        match self {
            NodeWidth::U16 => 0xFFFF,
            NodeWidth::U24 => 0x00FF_FFFF,
        }
    }

    /// Total bytes per node record. Driven by the (larger) element layout:
    /// `tag(1) + 5 link slots + 3 store slots (2 bytes each)` = `7 + 5*slot`.
    /// (`U16` → 17, `U24` → 22.) Text/comment nodes fit within this stride.
    const fn node_size(self) -> usize {
        7 + 5 * self.slot()
    }

    const fn parent(self) -> usize {
        1
    }
    const fn prev(self) -> usize {
        1 + self.slot()
    }
    const fn next(self) -> usize {
        1 + 2 * self.slot()
    }
    const fn first(self) -> usize {
        1 + 3 * self.slot()
    }
    const fn last(self) -> usize {
        1 + 4 * self.slot()
    }
    const fn class(self) -> usize {
        1 + 5 * self.slot()
    }
    const fn attr(self) -> usize {
        1 + 5 * self.slot() + 2
    }
    const fn data(self) -> usize {
        1 + 5 * self.slot() + 4
    }
    // Text/comment nodes overlay a u32 start/end onto the first-child slot region.
    const fn text_start(self) -> usize {
        1 + 3 * self.slot()
    }
    const fn text_end(self) -> usize {
        1 + 3 * self.slot() + 4
    }
}

/// Save as `U16` only with a 10% margin under the `u16` ceiling (= 58,981), so a
/// loaded compact document keeps edit headroom before it would need `U24`.
const DOWNPACK_MARGIN: usize = (0xFFFF * 9) / 10;

// ---- width-aware byte (un)packing over a flat node blob ----

fn read_node_slot(bytes: &[u8], pos: usize, width: NodeWidth) -> Option<NodeIndex> {
    // explicit fixed-size reads per width let the compiler emit a single load
    // rather than a byte-at-a-time loop (this is the read hot path).
    let v = match width {
        NodeWidth::U16 => u16::from_le_bytes([bytes[pos], bytes[pos + 1]]) as u32,
        NodeWidth::U24 => {
            (bytes[pos] as u32) | ((bytes[pos + 1] as u32) << 8) | ((bytes[pos + 2] as u32) << 16)
        }
    };
    if v == width.sentinel() {
        None
    } else {
        Some(NodeIndex::new(v))
    }
}

fn write_node_slot(bytes: &mut [u8], pos: usize, width: NodeWidth, val: Option<NodeIndex>) {
    let v = match val {
        Some(n) => n.as_u32(),
        None => width.sentinel(),
    };
    match width {
        NodeWidth::U16 => {
            bytes[pos] = v as u8;
            bytes[pos + 1] = (v >> 8) as u8;
        }
        NodeWidth::U24 => {
            bytes[pos] = v as u8;
            bytes[pos + 1] = (v >> 8) as u8;
            bytes[pos + 2] = (v >> 16) as u8;
        }
    }
}

fn read_u16_slot(bytes: &[u8], pos: usize) -> Option<u16> {
    opt_u16([bytes[pos], bytes[pos + 1]])
}

fn write_u16_slot(bytes: &mut [u8], pos: usize, val: Option<u16>) {
    let v = val.unwrap_or(u16::MAX);
    bytes[pos] = v as u8;
    bytes[pos + 1] = (v >> 8) as u8;
}

fn read_u32_slot(bytes: &[u8], pos: usize) -> u32 {
    u32::from_le_bytes([bytes[pos], bytes[pos + 1], bytes[pos + 2], bytes[pos + 3]])
}

fn write_u32_slot(bytes: &mut [u8], pos: usize, v: u32) {
    bytes[pos..pos + 4].copy_from_slice(&v.to_le_bytes());
}

/// Encapsulates a byte vector, manipulated by index, where each index represents a
/// node in the tree. `width` records whether node links are packed 2 or 3 bytes wide.
#[derive(Archive, Deserialize, Serialize, PartialEq, Hash, Clone)]
pub(crate) struct Nodes {
    /// Node-link width: [`WIDTH_U16`] or [`WIDTH_U24`].
    width: u8,
    bytes: Vec<u8>,
}

/// A borrowed, read-only view over the node-topology blob.
///
/// The blob is a flat `[u8]` of fixed-width node records read with `from_le_bytes`,
/// so the exact same view serves the owned `Nodes` (via [`Nodes::view`]) and the
/// rkyv-archived `ArchivedNodes` (whose `ArchivedVec<u8>` derefs to the
/// byte-identical `&[u8]`). This is what makes zero-copy querying of an mmap'd
/// archive possible without re-parsing or deserializing — at either width.
#[derive(Clone, Copy)]
pub(crate) struct NodesView<'a> {
    width: NodeWidth,
    bytes: &'a [u8],
}

impl<'a> NodesView<'a> {
    #[cfg(test)]
    pub(crate) fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    fn base(&self, index: NodeIndex) -> usize {
        index.as_usize() * self.width.node_size()
    }

    pub fn len(&self) -> usize {
        self.bytes.len() / self.width.node_size()
    }

    pub fn is_string_node(&self, index: NodeIndex) -> bool {
        let tag = self.tag(index);
        tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment
    }

    pub fn is_inline_element(&self, index: NodeIndex) -> bool {
        self.tag(index).is_inline_element()
    }

    pub fn is_block_element(&self, index: NodeIndex) -> bool {
        self.tag(index).is_block_element()
    }

    pub fn tag(&self, index: NodeIndex) -> HtmlTag {
        HtmlTag::from_repr(self.bytes[self.base(index)]).unwrap()
    }

    pub fn parent_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        read_node_slot(
            self.bytes,
            self.base(index) + self.width.parent(),
            self.width,
        )
    }

    pub fn prev_sibling_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        read_node_slot(self.bytes, self.base(index) + self.width.prev(), self.width)
    }

    pub fn next_sibling_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        read_node_slot(self.bytes, self.base(index) + self.width.next(), self.width)
    }

    pub fn first_child_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        match self.is_string_node(index) {
            true => None,
            false => read_node_slot(
                self.bytes,
                self.base(index) + self.width.first(),
                self.width,
            ),
        }
    }

    pub fn last_child_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        match self.is_string_node(index) {
            true => None,
            false => read_node_slot(self.bytes, self.base(index) + self.width.last(), self.width),
        }
    }

    pub fn text_range(&self, index: NodeIndex) -> Range<u32> {
        debug_assert!(self.is_string_node(index));
        let base = self.base(index);
        let start = read_u32_slot(self.bytes, base + self.width.text_start());
        let end = read_u32_slot(self.bytes, base + self.width.text_end());
        start..end
    }

    pub fn class_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            read_u16_slot(self.bytes, self.base(index) + self.width.class()).map(ListIndex::from)
        }
    }

    pub fn attr_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            read_u16_slot(self.bytes, self.base(index) + self.width.attr()).map(ListIndex::from)
        }
    }

    pub fn data_attr_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        if self.is_string_node(index) {
            None
        } else {
            read_u16_slot(self.bytes, self.base(index) + self.width.data()).map(ListIndex::from)
        }
    }

    pub(crate) fn dbg_table_string(&self, index: NodeIndex) -> String {
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

    fn parent_list(&self, mut index: NodeIndex) -> Vec<String> {
        let mut list = Vec::new();
        while let Some(parent) = self.parent_index(index) {
            list.push(format!("{parent:<2} > "));
            index = parent;
        }
        list.reverse();
        list
    }
}

impl Default for Nodes {
    fn default() -> Self {
        Self::new()
    }
}

impl Nodes {
    /// create a new nodevec with a root node (always at u24 width)
    pub fn new() -> Self {
        let mut me = Self {
            width: NodeWidth::U24.as_u8(),
            bytes: Vec::new(),
        };
        me.add_node(HtmlTag::sys_root, None, None, None);
        me
    }

    pub(crate) fn new_based_on(nodes: &Nodes) -> Self {
        let bytes = Vec::with_capacity(nodes.bytes.len());
        Self {
            width: NodeWidth::U24.as_u8(),
            bytes,
        }
    }

    fn width(&self) -> NodeWidth {
        NodeWidth::from_u8(self.width)
    }

    fn base(&self, index: NodeIndex) -> usize {
        index.as_usize() * self.width().node_size()
    }

    /// A borrowed read-only view over the node blob. All read accessors live on
    /// [`NodesView`] so the identical logic serves both the owned `Vec<u8>` and the
    /// rkyv-archived `ArchivedVec<u8>` (which is byte-identical).
    pub(crate) fn view(&self) -> NodesView<'_> {
        NodesView {
            width: self.width(),
            bytes: &self.bytes,
        }
    }

    pub fn len(&self) -> usize {
        self.view().len()
    }

    pub fn is_string_node(&self, index: NodeIndex) -> bool {
        self.view().is_string_node(index)
    }

    pub fn is_inline_element(&self, index: NodeIndex) -> bool {
        self.view().is_inline_element(index)
    }

    pub fn is_block_element(&self, index: NodeIndex) -> bool {
        self.view().is_block_element(index)
    }

    pub fn tag(&self, index: NodeIndex) -> HtmlTag {
        self.view().tag(index)
    }

    pub fn parent_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        self.view().parent_index(index)
    }

    pub fn prev_sibling_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        self.view().prev_sibling_index(index)
    }

    pub fn next_sibling_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        self.view().next_sibling_index(index)
    }

    pub fn first_child_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        self.view().first_child_index(index)
    }

    pub fn last_child_index(&self, index: NodeIndex) -> Option<NodeIndex> {
        self.view().last_child_index(index)
    }

    pub fn class_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        self.view().class_list_index(index)
    }

    pub fn attr_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        self.view().attr_list_index(index)
    }

    pub fn data_attr_list_index(&self, index: NodeIndex) -> Option<ListIndex> {
        self.view().data_attr_list_index(index)
    }

    pub fn add_as_next_sibling(&mut self, index: NodeIndex, tag: HtmlTag) -> NodeIndex {
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

    pub fn add_as_prev_sibling(&mut self, index: NodeIndex, tag: HtmlTag) -> NodeIndex {
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

    pub fn add_as_first_child(&mut self, index: NodeIndex, tag: HtmlTag) -> NodeIndex {
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

    pub fn add_as_last_child(&mut self, index: NodeIndex, tag: HtmlTag) -> NodeIndex {
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

    fn join(&mut self, prev_sibling: NodeIndex, next_sibling: NodeIndex) {
        self.set_prev_sibling_index(next_sibling, Some(prev_sibling));
        self.set_next_sibling_index(prev_sibling, Some(next_sibling));
    }

    fn update_children_parent(&mut self, first_child: NodeIndex, parent: NodeIndex) {
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
    pub fn remove_children(&mut self, index: NodeIndex) {
        self.set_first_child_index(index, None);
        self.set_last_child_index(index, None);
    }

    /// Remove a node by removing all references to it and removing
    /// it's parent reference. The node itself keeps the current
    /// sibling references which is useful when iterating
    pub fn remove(&mut self, index: NodeIndex) -> Option<NodeIndex> {
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

    pub fn unwrap_node(&mut self, index: NodeIndex) -> Option<NodeIndex> {
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
    pub fn replace_with(&mut self, index: NodeIndex, new_index: NodeIndex) {
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

    fn set_node_at(&mut self, index: NodeIndex, offset: usize, value: Option<NodeIndex>) {
        let width = self.width();
        let pos = self.base(index) + offset;
        write_node_slot(&mut self.bytes, pos, width, value);
    }

    fn set_u16_at(&mut self, index: NodeIndex, offset: usize, value: Option<u16>) {
        let pos = self.base(index) + offset;
        write_u16_slot(&mut self.bytes, pos, value);
    }

    pub(crate) fn set_parent_index(&mut self, index: NodeIndex, value: Option<NodeIndex>) {
        let offset = self.width().parent();
        self.set_node_at(index, offset, value);
    }

    pub(crate) fn set_prev_sibling_index(&mut self, index: NodeIndex, value: Option<NodeIndex>) {
        let offset = self.width().prev();
        self.set_node_at(index, offset, value);
    }

    pub(crate) fn set_next_sibling_index(&mut self, index: NodeIndex, value: Option<NodeIndex>) {
        let offset = self.width().next();
        self.set_node_at(index, offset, value);
    }

    pub(crate) fn set_first_child_index(&mut self, index: NodeIndex, value: Option<NodeIndex>) {
        debug_assert!(!self.is_string_node(index));
        let offset = self.width().first();
        self.set_node_at(index, offset, value);
    }

    pub(crate) fn set_last_child_index(&mut self, index: NodeIndex, value: Option<NodeIndex>) {
        debug_assert!(!self.is_string_node(index));
        let offset = self.width().last();
        self.set_node_at(index, offset, value);
    }

    pub(crate) fn set_class_list_index(&mut self, index: NodeIndex, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        let offset = self.width().class();
        self.set_u16_at(index, offset, value);
    }

    pub(crate) fn set_attr_list_index(&mut self, index: NodeIndex, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        let offset = self.width().attr();
        self.set_u16_at(index, offset, value);
    }

    pub(crate) fn set_data_attr_list_index(&mut self, index: NodeIndex, value: Option<u16>) {
        debug_assert!(!self.is_string_node(index));
        let offset = self.width().data();
        self.set_u16_at(index, offset, value);
    }

    pub(crate) fn set_text_range(&mut self, index: NodeIndex, range: Range<u32>) {
        debug_assert!(self.is_string_node(index));
        let width = self.width();
        let base = self.base(index);
        write_u32_slot(&mut self.bytes, base + width.text_start(), range.start);
        write_u32_slot(&mut self.bytes, base + width.text_end(), range.end);
    }

    pub(crate) fn add_node(
        &mut self,
        tag: HtmlTag,
        parent_index: Option<NodeIndex>,
        prev_sibling: Option<NodeIndex>,
        next_sibling: Option<NodeIndex>,
    ) -> NodeIndex {
        let width = self.width();
        let node_size = width.node_size();
        let new = self.bytes.len() / node_size;
        assert!(
            (new as u64) < width.sentinel() as u64,
            "htmlarc: document exceeds the maximum of {} nodes",
            width.sentinel()
        );
        let index = NodeIndex::new(new as u32);

        // append a zeroed record, then fill it; node-link slots must be written
        // explicitly because a zeroed slot would read back as node 0, not `None`.
        self.bytes.resize(self.bytes.len() + node_size, 0);
        let base = self.base(index);
        self.bytes[base] = tag as u8;
        self.set_parent_index(index, parent_index);
        self.set_prev_sibling_index(index, prev_sibling);
        self.set_next_sibling_index(index, next_sibling);
        if tag == HtmlTag::sys_text || tag == HtmlTag::sys_comment {
            self.set_text_range(index, 0..0);
        } else {
            self.set_first_child_index(index, None);
            self.set_last_child_index(index, None);
            self.set_class_list_index(index, None);
            self.set_attr_list_index(index, None);
            self.set_data_attr_list_index(index, None);
        }
        index
    }

    /// Choose the most compact on-disk node width. Owned/edited documents are always
    /// built at u24; this is called at serialize time to down-pack small documents to
    /// u16 (`count <= 58,981`). Documents above the margin (or already u16) are
    /// returned unchanged. The owned form is unaffected — only the serialized copy.
    pub(crate) fn into_optimal_width(self) -> Nodes {
        if self.width() == NodeWidth::U24 && self.len() <= DOWNPACK_MARGIN {
            let bytes = repack(self.view(), NodeWidth::U16);
            Nodes {
                width: NodeWidth::U16.as_u8(),
                bytes,
            }
        } else {
            self
        }
    }

    pub(crate) fn dbg_table_string(&self, index: NodeIndex) -> String {
        self.view().dbg_table_string(index)
    }
}

/// Re-pack a node blob from its current width into `dst` width. Reads every field
/// through the (width-aware) source view and writes it at the destination layout.
fn repack(src: NodesView, dst: NodeWidth) -> Vec<u8> {
    let n = src.len();
    let mut out = vec![0u8; n * dst.node_size()];
    for i in 0..n {
        let idx = NodeIndex::new(i as u32);
        let base = i * dst.node_size();
        out[base] = src.tag(idx) as u8;
        write_node_slot(&mut out, base + dst.parent(), dst, src.parent_index(idx));
        write_node_slot(
            &mut out,
            base + dst.prev(),
            dst,
            src.prev_sibling_index(idx),
        );
        write_node_slot(
            &mut out,
            base + dst.next(),
            dst,
            src.next_sibling_index(idx),
        );
        if src.is_string_node(idx) {
            let r = src.text_range(idx);
            write_u32_slot(&mut out, base + dst.text_start(), r.start);
            write_u32_slot(&mut out, base + dst.text_end(), r.end);
        } else {
            write_node_slot(
                &mut out,
                base + dst.first(),
                dst,
                src.first_child_index(idx),
            );
            write_node_slot(&mut out, base + dst.last(), dst, src.last_child_index(idx));
            write_u16_slot(
                &mut out,
                base + dst.class(),
                src.class_list_index(idx).map(|l| l.as_u16()),
            );
            write_u16_slot(
                &mut out,
                base + dst.attr(),
                src.attr_list_index(idx).map(|l| l.as_u16()),
            );
            write_u16_slot(
                &mut out,
                base + dst.data(),
                src.data_attr_list_index(idx).map(|l| l.as_u16()),
            );
        }
    }
    out
}

impl ArchivedNodes {
    /// Zero-copy view over the archived node blob — the `ArchivedVec<u8>` derefs to
    /// the same `&[u8]` the owned path uses, so query code is representation-agnostic.
    pub(crate) fn view(&self) -> NodesView<'_> {
        NodesView {
            width: NodeWidth::from_u8(self.width),
            bytes: &self.bytes,
        }
    }
}

impl Debug for Nodes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "idx  prev-sib, next-sib, first-ch, last-ch,  parents and tag"
        )?;
        for i in 0..self.len() {
            writeln!(f, "{}", self.dbg_table_string(NodeIndex::new(i as u32)))?;
        }
        Ok(())
    }
}

fn dbg_w(val: Option<NodeIndex>, width: usize) -> String {
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
    assert_eq!(vec.tag(NodeIndex::ROOT), HtmlTag::sys_root);
    assert_eq!(vec.parent_index(NodeIndex::ROOT), None);
    assert_eq!(vec.prev_sibling_index(NodeIndex::ROOT), None);
    assert_eq!(vec.next_sibling_index(NodeIndex::ROOT), None);
    assert_eq!(vec.first_child_index(NodeIndex::ROOT), None);
    assert_eq!(vec.last_child_index(NodeIndex::ROOT), None);
    assert_eq!(vec.class_list_index(NodeIndex::ROOT), None);
    assert_eq!(vec.attr_list_index(NodeIndex::ROOT), None);

    let index = vec.add_node(
        HtmlTag::abbr,
        Some(NodeIndex::new(1)),
        Some(NodeIndex::new(2)),
        Some(NodeIndex::new(3)),
    );
    vec.set_first_child_index(index, Some(NodeIndex::new(4)));
    vec.set_last_child_index(index, Some(NodeIndex::new(5)));

    println!("{:?}", vec.bytes);
    assert_eq!(index, NodeIndex::new(1));
    assert_eq!(vec.tag(NodeIndex::new(1)), HtmlTag::abbr);
    assert_eq!(vec.parent_index(NodeIndex::new(1)), Some(NodeIndex::new(1)));
    assert_eq!(
        vec.prev_sibling_index(NodeIndex::new(1)),
        Some(NodeIndex::new(2))
    );
    assert_eq!(
        vec.next_sibling_index(NodeIndex::new(1)),
        Some(NodeIndex::new(3))
    );
    assert_eq!(
        vec.first_child_index(NodeIndex::new(1)),
        Some(NodeIndex::new(4))
    );
    assert_eq!(
        vec.last_child_index(NodeIndex::new(1)),
        Some(NodeIndex::new(5))
    );
}

#[test]
fn test_adaptive_width_roundtrip() {
    // small tree built at u24 down-packs to u16 losslessly
    let mut u24 = Nodes::new();
    let body = u24.add_as_last_child(NodeIndex::ROOT, HtmlTag::body);
    let div = u24.add_as_last_child(body, HtmlTag::div);
    let p = u24.add_as_last_child(div, HtmlTag::p);
    u24.add_as_last_child(body, HtmlTag::span);

    let u16 = u24.clone().into_optimal_width();
    assert_eq!(u16.width(), NodeWidth::U16);
    assert_eq!(u16.len(), u24.len());
    // identical topology at both widths
    for i in 0..u24.len() as u32 {
        let idx = NodeIndex::new(i);
        assert_eq!(u24.tag(idx), u16.tag(idx));
        assert_eq!(u24.parent_index(idx), u16.parent_index(idx));
        assert_eq!(u24.prev_sibling_index(idx), u16.prev_sibling_index(idx));
        assert_eq!(u24.next_sibling_index(idx), u16.next_sibling_index(idx));
        assert_eq!(u24.first_child_index(idx), u16.first_child_index(idx));
        assert_eq!(u24.last_child_index(idx), u16.last_child_index(idx));
    }
    // u16 records are smaller
    assert!(u16.bytes.len() < u24.bytes.len());
    assert_eq!(p.as_u32(), 3);
}
