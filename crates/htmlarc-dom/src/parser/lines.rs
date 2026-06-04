use std::ops::Range;

pub struct Lines {
    newlines: Vec<usize>,
    len: usize,
}

impl Lines {
    pub fn new(s: &str) -> Self {
        let newlines = s
            .char_indices()
            .filter(|(_, c)| *c == '\n')
            .map(|(i, _)| i)
            .collect();
        let len = s.len();
        Self { newlines, len }
    }

    fn line_at(&self, index: usize) -> Line {
        let start = if index == 0 || self.newlines.is_empty() {
            0
        } else {
            let mut pos = self.newlines[index - 1];
            if pos + 1 < self.len {
                pos += 1;
            }
            pos
        };
        let end = if index < self.newlines.len() {
            self.newlines[index]
        } else {
            self.len
        };
        Line::new(start..end, index)
    }

    fn line_before(&self, index: usize) -> Option<Line> {
        (index > 0).then(|| self.line_at(index - 1))
    }

    fn line_index_for(&self, pos: usize) -> usize {
        for (i, &end) in self.newlines.iter().enumerate() {
            if pos < end {
                return i;
            }
        }
        self.newlines.len()
    }

    pub fn line_for_pos(&self, pos: usize) -> (Line, Option<Line>) {
        let index = self.line_index_for(pos);
        (self.line_at(index), self.line_before(index))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Line {
    pub range: Range<usize>,
    pub num: usize,
}

impl Line {
    fn new(range: Range<usize>, index: usize) -> Self {
        Self {
            range,
            num: index + 1,
        }
    }
}

#[test]
fn test_single_line() {
    let string = "aa";

    let (line, prev) = Lines::new(string).line_for_pos(1);
    assert_eq!(&string[line.range], "aa");
    assert_eq!(line.num, 1);
    assert!(prev.is_none());

    let string = "a";

    let (line, prev) = Lines::new(string).line_for_pos(1);
    assert_eq!(&string[line.range], "a");
    assert_eq!(line.num, 1);
    assert!(prev.is_none());

    let string = "";

    let (line, prev) = Lines::new(string).line_for_pos(1);
    assert_eq!(&string[line.range], "");
    assert_eq!(line.num, 1);
    assert!(prev.is_none());
}

#[test]
fn test_multiple_lines() {
    let string = "aa\nbbb\ncc";
    // indexes    123 4567 89

    let (line, prev) = Lines::new(string).line_for_pos(1);
    assert_eq!(&string[line.range], "aa");
    assert_eq!(line.num, 1);
    assert!(prev.is_none());

    let (line, prev) = Lines::new(string).line_for_pos(5);
    assert_eq!(&string[line.range], "bbb");
    assert_eq!(line.num, 2);
    assert!(prev.is_some());
    let prev = prev.unwrap();
    assert_eq!(&string[prev.range], "aa");
    assert_eq!(prev.num, 1);

    let (line, prev) = Lines::new(string).line_for_pos(8);
    assert_eq!(&string[line.range], "cc");
    assert_eq!(line.num, 3);
    assert!(prev.is_some());
    let prev = prev.unwrap();
    assert_eq!(&string[prev.range], "bbb");
    assert_eq!(prev.num, 2);
}
