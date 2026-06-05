use std::fmt::{self, Debug, Display};

/// Index of a node within a document's flat node array.
///
/// A newtype over `u32` so the compiler keeps node indices distinct from the
/// `u16` store/list indices (`ListIndex`, `ClassIndex`, ...). Node indices are
/// never serialized directly — they live encoded (u16 or u24) inside the node
/// byte blob — so this type deliberately does **not** derive any rkyv traits.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NodeIndex(u32);

impl NodeIndex {
    /// The document root is always node 0.
    pub const ROOT: NodeIndex = NodeIndex(0);

    #[inline]
    pub const fn new(value: u32) -> Self {
        NodeIndex(value)
    }

    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }
}

impl From<u16> for NodeIndex {
    #[inline]
    fn from(value: u16) -> Self {
        NodeIndex(value as u32)
    }
}

impl From<u32> for NodeIndex {
    #[inline]
    fn from(value: u32) -> Self {
        NodeIndex(value)
    }
}

impl From<NodeIndex> for usize {
    #[inline]
    fn from(value: NodeIndex) -> Self {
        value.0 as usize
    }
}

// Delegating Debug/Display so format specs (width/alignment) used by the node
// debug table still apply.
impl Debug for NodeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl Display for NodeIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}
