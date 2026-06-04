use std::{
    ops::{Bound, RangeBounds},
    str::CharIndices,
};

pub struct CssChars<'s> {
    string: &'s str,
    iter: CharIndices<'s>,
    current: Option<(usize, char)>,
    last_index: usize,
}

impl<'s> CssChars<'s> {
    pub fn new(string: &'s str) -> Self {
        let mut iter = string.char_indices();

        if let Some((i, c)) = iter.next() {
            Self {
                string,
                iter,
                current: Some((i, c)),
                last_index: i,
            }
        } else {
            Self {
                string,
                iter,
                current: None,
                last_index: 0,
            }
        }
    }

    pub fn current(&self) -> Option<(usize, char)> {
        self.current
    }

    pub fn last_index(&self) -> usize {
        self.last_index
    }

    pub fn str<R>(&self, range: R) -> &'s str
    where
        R: RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => self.string.len(),
        };

        &self.string[start..end]
    }

    pub fn skip_spaces(&mut self) -> Option<(usize, char)> {
        while let Some((i, c)) = self.current() {
            if !c.is_whitespace() {
                return Some((i, c));
            } else {
                self.next();
            }
        }
        None
    }

    fn update(&mut self, new: Option<(usize, char)>) -> Option<(usize, char)> {
        if let Some((i, c)) = new {
            self.current = Some((i, c));
            self.last_index = i;

            Some((i, c))
        } else {
            self.current = None;
            None
        }
    }
}

impl Iterator for CssChars<'_> {
    type Item = (usize, char);

    fn next(&mut self) -> Option<Self::Item> {
        let new = self.iter.next();
        self.update(new)
    }
}

#[test]
fn test_css_chars() {
    let mut chars = CssChars::new("abc");

    assert_eq!(chars.current(), Some((0, 'a')));
    assert_eq!(chars.next(), Some((1, 'b')));
    assert_eq!(chars.current(), Some((1, 'b')));
    assert_eq!(chars.next(), Some((2, 'c')));

    let chars = CssChars::new("");

    assert_eq!(chars.current(), None);
}
