use crate::{dom::DomInner, html::HtmlElement};

use super::{
    queue_iter::QueueIter,
    tag_iter::{ElementStage, TagIter, TagStage},
};

/// Keeps track of uninterrupted sequences that can be inlined
pub struct Inliner<'dom> {
    iter: TagIter<'dom>,
    /// The candidates are elements that are inlineable and accumulated
    /// until a non-inlineable element is found (or none are left).
    /// When one is found, the candidates are examined and all that
    /// have a depth bigger than or equal to the one of the non-inlineable
    /// element are inlined.
    /// Before returning the inlineable elements, the previous candidates are
    /// returned.
    queued: Option<QueueIter>,
}

impl<'dom> Inliner<'dom> {
    pub fn new(dom: &'dom DomInner, index: u16) -> Self {
        Self {
            iter: TagIter::new(dom, index),
            queued: None,
        }
    }

    fn is_inlineable(&self, stage: &ElementStage) -> bool {
        let el = HtmlElement::new(self.iter.dom, stage.index);
        el.is_format_inlined()
    }

    fn next_inner(&mut self) -> Option<(ElementStage, bool)> {
        if let Some(ref mut queue) = self.queued {
            if let Some(val) = queue.next() {
                return Some(val);
            } else {
                self.queued = None;
            }
        }
        let next = self.iter.next()?;

        let (elem, inlined) = if self.is_inlineable(&next) && next.stage == TagStage::Open {
            let mut vec = vec![next];
            let mut last: Option<ElementStage> = None;
            while let Some(next) = self.iter.next() {
                if self.is_inlineable(&next) {
                    vec.push(next);
                } else {
                    last = Some(next);
                    break;
                }
            }
            let mut q = QueueIter::new(vec, last);
            let next = q.next()?;
            self.queued = Some(q);
            next
        } else {
            (next, false)
        };
        Some((elem, inlined))
    }
}

impl<'dom> Iterator for Inliner<'dom> {
    type Item = ElementInfo<'dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_inner().map(|(stage, inlined)| ElementInfo {
            element: HtmlElement::new(self.iter.dom, stage.index),
            in_inline_sequence: inlined,
            stage: stage.stage,
            depth: stage.depth,
        })
    }
}

pub struct ElementInfo<'dom> {
    pub element: HtmlElement<'dom, DomInner>,
    pub stage: TagStage,
    pub in_inline_sequence: bool,
    pub depth: u16,
}
#[cfg(test)]
use crate::prelude::*;

#[cfg(test)]
fn inline_vec(html: &str, index: u16) -> Vec<(HtmlTag, TagStage, bool, u16)> {
    let dom = HtmlDoc::parse(html).unwrap().inner();
    Inliner::new(&dom, index)
        .map(|info| {
            (
                info.element.tag(),
                info.stage,
                info.in_inline_sequence,
                info.depth,
            )
        })
        .collect()
}

#[test]
fn test() {
    use super::tag_iter::TagStage::*;

    let html = r#"<div><i>hello</i><span>there</span></div>"#;

    let out = inline_vec(html, 0);
    assert_eq!(
        out,
        vec![
            (HtmlTag::div, Open, false, 0),
            (HtmlTag::i, Open, true, 1),
            (HtmlTag::sys_text, Open, true, 2),
            (HtmlTag::sys_text, Close, true, 2),
            (HtmlTag::i, Close, true, 1),
            (HtmlTag::span, Open, false, 1),
            (HtmlTag::sys_text, Open, true, 2),
            (HtmlTag::sys_text, Close, true, 2),
            (HtmlTag::span, Close, false, 1),
            (HtmlTag::div, Close, false, 0),
        ]
    );
}

#[test]
fn case1() {
    use super::tag_iter::TagStage::*;

    let html = r#"<li><sup><a>(de)</a></sup><b>f</b></li>"#;

    let out = inline_vec(html, 0);
    assert_eq!(
        out,
        vec![
            (HtmlTag::li, Open, false, 0),
            (HtmlTag::sup, Open, false, 1),
            (HtmlTag::a, Open, false, 2),
            (HtmlTag::sys_text, Open, true, 3),
            (HtmlTag::sys_text, Close, true, 3),
            (HtmlTag::a, Close, false, 2),
            (HtmlTag::sup, Close, false, 1),
            (HtmlTag::b, Open, true, 1),
            (HtmlTag::sys_text, Open, true, 2),
            (HtmlTag::sys_text, Close, true, 2),
            (HtmlTag::b, Close, true, 1),
            (HtmlTag::li, Close, false, 0),
        ]
    );
}
