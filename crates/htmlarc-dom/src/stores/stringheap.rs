use std::ops::{Index, Range};

use rkyv::{Archive, Deserialize, Serialize};

/// A string heap: strings can be added and found on their index. An
/// existing string can be modified in-place if the new string has
/// the same size or is smaller.
///
/// Tye strings are efficiently stored but is limited to 2^16 bytes of storage.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct StringHeap {
    /// The **end** position of a string is stored here. This means that
    /// the first stored string has the range of `0..positions[0]` and that
    /// the n-th string has the range of `positions[n-1]..positions[n]`.
    positions: Vec<u32>,
    strings: Vec<u8>,
}

impl StringHeap {
    pub fn with_capacity(bytes: usize, count: usize) -> Self {
        Self {
            positions: Vec::with_capacity(count),
            strings: Vec::with_capacity(bytes),
        }
    }

    pub fn with_capacity_as(other: &Self) -> Self {
        Self {
            positions: Vec::with_capacity(other.positions.len()),
            strings: Vec::with_capacity(other.strings.len()),
        }
    }

    pub fn insert(&mut self, s: &str) -> u16 {
        let end_pos = (self.strings.len() + s.len()) as u32;
        self.positions.push(end_pos);
        self.strings.extend_from_slice(s.as_bytes());
        self.positions.len() as u16 - 1
    }

    pub fn len(&self) -> u16 {
        self.positions.len() as u16
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    pub(crate) fn view(&self) -> StringHeapView<'_> {
        StringHeapView {
            positions: PositionsView::Owned(self.positions.as_slice()),
            strings: &self.strings,
        }
    }
}

/// The position table: owned native `u32`s, or archived little-endian `u32`s read
/// via `.to_native()`. Erasing this distinction is what lets one view serve both.
#[derive(Clone, Copy)]
enum PositionsView<'a> {
    Owned(&'a [u32]),
    Archived(&'a [rkyv::Archived<u32>]),
}

impl PositionsView<'_> {
    #[cfg(test)]
    fn len(&self) -> usize {
        match self {
            Self::Owned(s) => s.len(),
            Self::Archived(s) => s.len(),
        }
    }

    fn at(&self, index: usize) -> u32 {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => s[index].to_native(),
        }
    }
}

/// Borrowed read-only view over a [`StringHeap`] — works over both the owned and
/// the rkyv-archived representation.
#[derive(Clone, Copy)]
pub(crate) struct StringHeapView<'a> {
    positions: PositionsView<'a>,
    strings: &'a [u8],
}

impl<'a> StringHeapView<'a> {
    #[cfg(test)]
    pub(crate) fn len(&self) -> u16 {
        self.positions.len() as u16
    }

    fn range(&self, index: u16) -> Range<usize> {
        let end = index as usize;
        if end == 0 {
            0..self.positions.at(0) as usize
        } else {
            self.positions.at(end - 1) as usize..self.positions.at(end) as usize
        }
    }

    pub(crate) fn get(&self, index: u16) -> &'a str {
        unsafe { std::str::from_utf8_unchecked(&self.strings[self.range(index)]) }
    }
}

impl ArchivedStringHeap {
    pub(crate) fn view(&self) -> StringHeapView<'_> {
        StringHeapView {
            positions: PositionsView::Archived(self.positions.as_slice()),
            strings: &self.strings,
        }
    }
}

impl Index<u16> for StringHeap {
    type Output = str;

    fn index(&self, index: u16) -> &Self::Output {
        self.view().get(index)
    }
}

#[test]
fn archived_stringheap_round_trip() {
    use rkyv::rancor::Error;

    let mut heap = StringHeap::default();
    for s in ["", "hi", "ho", "a longer one", "x"] {
        heap.insert(s);
    }

    let bytes = rkyv::to_bytes::<Error>(&heap).unwrap();
    let archived = rkyv::access::<ArchivedStringHeap, Error>(&bytes[..]).unwrap();

    // The archived u32 position table, read via .to_native(), reproduces the owned
    // Index<u16> exactly.
    assert_eq!(archived.view().len(), heap.len());
    for i in 0..heap.len() {
        assert_eq!(
            archived.view().get(i),
            &heap[i],
            "entry {i} matches zero-copy"
        );
    }
}

#[test]
fn string_heap() {
    let mut heap = StringHeap::default();

    assert_eq!(0, heap.len());

    let empty_string = heap.insert("");
    let hi_string = heap.insert("hi");
    let ho_string = heap.insert("ho");

    assert_eq!("", &heap[empty_string]);
    assert_eq!("hi", &heap[hi_string]);
    assert_eq!("ho", &heap[ho_string]);

    assert_eq!(3, heap.len());

    let list = (0..heap.len())
        .map(|i| &heap[i])
        .collect::<Vec<_>>()
        .join(", ");
    assert_eq!(", hi, ho", list);
}
