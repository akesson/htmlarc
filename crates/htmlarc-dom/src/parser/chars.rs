use super::lines::Lines;
use crate::error::{HtmlParseResult, HtmlParseError};
use std::{ops::Range, str::CharIndices};

#[derive(Debug)]
pub struct Chars<'a> {
    string: &'a str,
    iter: CharIndices<'a>,
    current: char,
    // current byte index which is also a valid char index
    index: usize,
    has_more: bool,
}
impl<'a> Chars<'a> {
    pub fn new(string: &'a str) -> Self {
        let mut iter = string.char_indices();
        let current = iter.next().map(|(_, c)| c).unwrap_or('\u{0}');

        Self {
            string,
            iter,
            current,
            index: 0,
            has_more: true,
        }
    }

    #[inline]
    fn update(&mut self, new: Option<(usize, char)>) -> Option<char> {
        if let Some((i, c)) = new {
            self.current = c;
            self.index = i;
            Some(c)
        } else {
            self.has_more = false;
            None
        }
    }

    pub fn current(&self) -> char {
        self.current
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn str(&self, range: Range<usize>) -> &'a str {
        &self.string[range]
    }

    pub fn str_from(&self, start: usize) -> &'a str {
        &self.string[start..self.index]
    }

    pub fn err<S: ToString>(&self, msg: S) -> HtmlParseError {
        let err_info = self.location_info();
        HtmlParseError::Parsing(format!("{}, at:\n{err_info}", msg.to_string()))
    }

    pub fn location_info(&self) -> String {
        let lines = Lines::new(self.string);
        let (line, prev) = lines.line_for_pos(self.index());

        let count = self.index().saturating_sub(line.range.start) + 7;
        let spaces = " ".repeat(count);
        let line_num = line.num;
        let line = &self.string[line.range];
        let mut s = format!("{line_num:>5}: {line}\n{spaces}^");

        if let Some(prev) = prev {
            let prev_num = prev.num;
            let prev = &self.string[prev.range];
            s = format!("{prev_num:>5}: {prev}\n{s}");
        }
        s
    }
    pub fn find_sequence<const N: usize>(&mut self, seq: [char; N]) -> HtmlParseResult<usize> {
        let mut start = self.index;
        let mut sequence_index = 0;
        let mut c = self.current;
        loop {
            if c == seq[sequence_index] {
                if sequence_index == 0 {
                    start = self.index;
                }
                sequence_index += 1
            } else {
                sequence_index = 0;
            }
            if sequence_index == N {
                return Ok(start);
            }
            let Some(next) = self.next() else { break };
            c = next;
        }
        Err(self.err(format!("Sequence not found: {:?}", seq)))
    }

    pub fn find<F: Fn(char) -> bool>(&mut self, cond: F) -> HtmlParseResult<usize> {
        if cond(self.current()) {
            return Ok(self.index);
        }
        while let Some(c) = self.next() {
            if cond(c) {
                return Ok(self.index);
            }
        }
        Err(self.err("Not found"))
    }

    pub fn assert_sequence<const N: usize>(&mut self, seq: [char; N]) -> HtmlParseResult<()> {
        let mut c = self.current;
        let mut sequence_index = 0;
        loop {
            if c == seq[sequence_index] {
                sequence_index += 1;
            } else {
                return Err(self.err(format!("Expected sequence {seq:?} not found")));
            }
            let Some(next) = self.next() else { break };
            c = next;
            if sequence_index == N {
                self.next();
                return Ok(());
            }
        }

        Err(self.err("Not found"))
    }

    pub fn assert_next<F: Fn(char) -> bool>(&mut self, cond: F) -> HtmlParseResult<()> {
        let Some(c) = self.next() else {
            return Err(self.err("Unexpected end"));
        };

        if !cond(c) {
            Err(self.err(format!("Unexpected character '{c}'")))
        } else {
            Ok(())
        }
    }

    /// assert current char
    pub fn assert_curr<F: Fn(char) -> bool>(&mut self, cond: F) -> HtmlParseResult<()> {
        let c = self.current;
        if !cond(c) {
            Err(self.err(format!("Unexpected character '{c}'")))
        } else {
            Ok(())
        }
    }

    pub fn skip_whitespaces(&mut self) {
        if self.current.is_whitespace() {
            while let Some(c) = self.next() {
                if !c.is_whitespace() {
                    return;
                }
            }
        }
    }

    pub fn next(&mut self) -> Option<char> {
        let new = self.iter.next();
        self.update(new)
    }

    pub fn next_skip_whitespaces(&mut self) -> Option<char> {
        while let Some(n) = self.next() {
            if !n.is_whitespace() {
                return Some(n);
            }
        }
        None
    }

    pub fn next_index(&mut self) -> Option<usize> {
        self.next().map(|_| self.index())
    }

    pub fn str_until<F: Fn(char) -> bool>(&mut self, from: usize, cond: F) -> HtmlParseResult<&'a str> {
        self.find(cond).map(|_| self.str_from(from))
    }

    #[cfg(test)]
    pub fn str_until_or_end<F: Fn(char) -> bool>(&mut self, cond: F) -> &'a str {
        let from = self.index;

        while let Some(c) = self.next() {
            if cond(c) {
                return self.str(from..self.index);
            }
        }
        &self.string[from..]
    }

    #[cfg(test)]
    pub fn str_remaining(&mut self) -> &str {
        &self.string[self.index()..]
    }
}

#[test]
fn test_iter_find() {
    let mut chars = Chars::new("a bb cc");
    chars.find(|c| c.is_whitespace()).unwrap();
    assert_eq!(chars.str_remaining(), " bb cc");
    chars.find(|c| c.is_whitespace()).unwrap();
    assert_eq!(chars.str_remaining(), " bb cc");

    chars.next();
    chars.find(|c| c.is_whitespace()).unwrap();
    assert_eq!(chars.str_remaining(), " cc");
}

#[test]
fn test_iter_skip() {
    let mut chars = Chars::new(" a b  c   d");
    assert_eq!(chars.current(), ' ');
    chars.skip_whitespaces();
    assert_eq!(chars.str_remaining(), "a b  c   d");
    chars.skip_whitespaces();
    assert_eq!(chars.str_remaining(), "a b  c   d");
    chars.next();
    chars.skip_whitespaces();
    assert_eq!(chars.str_remaining(), "b  c   d");
    chars.next();
    chars.skip_whitespaces();
    assert_eq!(chars.str_remaining(), "c   d");
    chars.next();
    chars.skip_whitespaces();
    assert_eq!(chars.str_remaining(), "d");
}

#[test]
fn test_iter_next_skip_whitespaces() {
    let mut chars = Chars::new(" a b  c   d");
    chars.next_skip_whitespaces();
    assert_eq!(chars.str_remaining(), "a b  c   d");
    chars.next_skip_whitespaces();
    assert_eq!(chars.str_remaining(), "b  c   d");
    chars.next_skip_whitespaces();
    assert_eq!(chars.str_remaining(), "c   d");
    chars.next_skip_whitespaces();
    assert_eq!(chars.str_remaining(), "d");
}

#[test]
fn test_iter_str_until() {
    let mut chars = Chars::new(" a b c");
    let s = chars.str_until(0, |c| c == 'b').unwrap();
    assert_eq!(s, " a ");
    assert_eq!(chars.str_remaining(), "b c");
}

#[test]
fn test_iter_str_until_or_end() {
    let mut chars = Chars::new(" a b c");
    let s = chars.str_until_or_end(|c| c == 'b');
    assert_eq!(s, " a ");
    assert_eq!(chars.str_remaining(), "b c");

    let mut chars = Chars::new("abc");
    let s = chars.str_until_or_end(|c| c.is_whitespace());
    assert_eq!(s, "abc");
}

#[test]
fn test_iter_find_sequence() {
    let mut chars = Chars::new("abcd--> ");
    let end = chars.find_sequence(['-', '-', '>']).unwrap();
    assert_eq!(chars.str(0..end), "abcd");
    assert_eq!(chars.current(), '>');
}

#[test]
fn test_iter_assert_sequence() {
    let mut chars = Chars::new("-->a");
    chars.assert_sequence(['-', '-', '>']).unwrap();
    assert_eq!(chars.current(), 'a');
}
