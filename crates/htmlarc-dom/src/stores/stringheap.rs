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

    #[inline]
    fn range(&self, index: u16) -> Range<usize> {
        let end = index as usize;
        if end == 0 {
            0..self.positions[0] as usize
        } else {
            self.positions[end - 1] as usize..self.positions[end] as usize
        }
    }
}

impl Index<u16> for StringHeap {
    type Output = str;

    fn index(&self, index: u16) -> &Self::Output {
        unsafe { std::str::from_utf8_unchecked(&self.strings[self.range(index)]) }
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
