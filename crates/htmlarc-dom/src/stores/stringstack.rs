use std::ops::{Index, Range};

use rkyv::{Archive, Deserialize, Serialize};

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

    pub fn push(&mut self, text: &str) -> Range<u32> {
        let start = self.strings.len() as u32;
        self.strings.extend_from_slice(text.as_bytes());
        start..(start + text.len() as u32)
    }

    pub fn size(&self) -> usize {
        self.strings.len()
    }
}

impl Index<Range<u32>> for StringStack {
    type Output = str;

    fn index(&self, range: Range<u32>) -> &Self::Output {
        unsafe {
            std::str::from_utf8_unchecked(&self.strings[range.start as usize..range.end as usize])
        }
    }
}
