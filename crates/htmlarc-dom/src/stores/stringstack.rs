use std::ops::{Index, Range};

use rkyv::{Archive, Deserialize, Serialize};

use super::StringSource;

/// The owned text/comment pool of one document: a flat UTF-8 `[u8]` that text nodes index by
/// `Range<u32>`. It is the *builder* (and live-edit) form — reads go through a
/// [`StringSource`], so a document is agnostic to whether its bytes are owned here, borrowed
/// from an mmap, or (later) inflated from a per-bundle compressed frame.
#[derive(Default, Archive, Serialize, Deserialize, Hash, Clone)]
pub struct StringStack {
    strings: Vec<u8>,
}

impl StringStack {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            strings: Vec::with_capacity(capacity),
        }
    }

    /// Wrap an already-built pool (e.g. a document's segment copied out of a per-bundle store
    /// when an archived document is materialised into an editable one).
    pub fn from_bytes(strings: Vec<u8>) -> Self {
        Self { strings }
    }

    pub fn push(&mut self, text: &str) -> Range<u32> {
        let start = self.strings.len() as u32;
        self.strings.extend_from_slice(text.as_bytes());
        start..(start + text.len() as u32)
    }

    pub fn size(&self) -> usize {
        self.strings.len()
    }

    /// Take the pool's bytes, leaving it empty. Used when serialising a document into a
    /// per-bundle store: the bytes move to the bundle and the per-document blob is left
    /// string-less.
    pub fn take_bytes(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.strings)
    }

    pub(crate) fn view(&self) -> StringSource<'_> {
        StringSource::plain(&self.strings)
    }
}

impl ArchivedStringStack {
    pub(crate) fn view(&self) -> StringSource<'_> {
        StringSource::plain(&self.strings)
    }
}

impl Index<Range<u32>> for StringStack {
    type Output = str;

    fn index(&self, range: Range<u32>) -> &Self::Output {
        self.view().get(range)
    }
}
