use crate::{error::IterationError, prelude::*};
use std::ops::{Range, RangeBounds};

pub struct Exactly<'dom, Dom, I>
where
    I: Iterator<Item = HtmlElement<'dom, Dom>>,
    Self: 'dom,
{
    iter: I,
    count: usize,
    range: Range<usize>,
    stop: bool,
}

impl<'dom, Dom, I> Exactly<'dom, Dom, I>
where
    Dom: DomRead,
    I: Iterator<Item = HtmlElement<'dom, Dom>>,
    Self: 'dom,
{
    #[cfg(test)]
    fn string(self) -> String {
        self.map(|exact| match exact {
            Ok(el) => format!("{}:{}", el.index(), el.tag()),
            Err(e) => e.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
    }

    pub(crate) fn new<R: RangeBounds<usize>>(iter: I, range: R) -> Self {
        let start = match range.start_bound() {
            std::ops::Bound::Included(&s) => s,
            std::ops::Bound::Excluded(&s) => s + 1,
            std::ops::Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            std::ops::Bound::Included(&s) => s + 1,
            std::ops::Bound::Excluded(&s) => s,
            std::ops::Bound::Unbounded => usize::MAX,
        };

        Self {
            iter,
            count: 0,
            range: Range { start, end },
            stop: false,
        }
    }
}

impl<'dom, Dom, I> Iterator for Exactly<'dom, Dom, I>
where
    I: Iterator<Item = HtmlElement<'dom, Dom>>,
    Self: 'dom,
{
    type Item = Result<HtmlElement<'dom, Dom>, IterationError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.stop {
            return None;
        }

        // return an error if the amount of elements found exceeds what we expected
        if self.count >= self.range.end {
            self.stop = true;
            return Some(Err(IterationError::Exceeds {
                max: self.range.end - 1,
                expected: self.range.clone(),
            }));
        }

        if let Some(el) = self.iter.next() {
            self.count += 1;
            return Some(Ok(el));
        }

        // return an error if the amount of elements is not enough
        if self.count < self.range.start {
            self.stop = true;
            return Some(Err(IterationError::Lacks {
                count: self.count,
                expected: self.range.clone(),
            }));
        }

        None
    }
}

#[test]
fn exactly_lacks() {
    let html_str = "<body><div>hi</div></body>";
    let html = HtmlDoc::parse(html_str).unwrap();
    let dom = html.dom();
    let iter = dom.root().descendants().set_include_text();
    let exactly = Exactly::new(iter, 4..6).string();

    assert_eq!(
        "1:body, 2:div, 3:text, [Expected 4..6, found only 3]",
        exactly
    );
}

#[test]
fn exactly_exceeds() {
    let html_str = "<body><div>hi</div></body>";
    let html = HtmlDoc::parse(html_str).unwrap();
    let dom = html.dom();
    let iter = dom.root().descendants().set_include_text();
    let exactly = Exactly::new(iter, 0..1).string();

    assert_eq!("1:body, [Expected 0..1, found more than 0]", exactly);
}

#[test]
fn exactly_within() {
    let html_str = "<body><div>hi</div></body>";
    let html = HtmlDoc::parse(html_str).unwrap();
    let dom = html.dom();
    let iter = dom.root().descendants().set_include_text();
    let exactly = Exactly::new(iter, 2..=3).string();

    assert_eq!("1:body, 2:div, 3:text", exactly);
}
