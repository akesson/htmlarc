use super::tag_iter::{ElementStage, TagStage};
use std::ops::Range;

#[cfg(test)]
use crate::dom::NodeIndex;

/// test helper: build an `ElementStage` from a plain index literal
#[cfg(test)]
fn es(index: u32, depth: u16, stage: TagStage) -> ElementStage {
    ElementStage::new(NodeIndex::new(index), depth, stage)
}

pub struct QueueIter {
    /// any element that is at same or higher depth than the non_inlineable element
    /// and also closed.
    inlineable: Range<usize>,
    vec: Vec<ElementStage>,
    pos: usize,
}
impl QueueIter {
    fn _new(inlineable: Range<usize>, vec: Vec<ElementStage>) -> Self {
        Self {
            inlineable,
            vec,
            pos: 0,
        }
    }

    pub fn new(mut vec: Vec<ElementStage>, non_inlineable: Option<ElementStage>) -> Self {
        let mut end = vec.len() - 1;
        // but only if they are closed
        while end > 0 && vec[end].stage == TagStage::Open {
            end -= 1;
        }

        if let Some(non_inlineable) = non_inlineable {
            vec.push(non_inlineable);
        }

        if end == 0 {
            return Self::_new(0..0, vec);
        }

        let start = if non_inlineable.is_some() {
            let d = vec[end].depth;
            // only element that are at the same depth of deeper than the non_inlineable element
            // are inlineable
            vec[..end]
                .iter()
                .position(|el| el.depth >= d)
                .unwrap_or(end)
        } else {
            0
        };
        Self::_new(start..end + 1, vec)
    }
}

impl Iterator for QueueIter {
    type Item = (ElementStage, bool);

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.vec.len() {
            return None;
        }
        let stage = self.vec[self.pos];
        let inlined = self.inlineable.contains(&self.pos);
        self.pos += 1;
        Some((stage, inlined))
    }
}

#[test]
fn queue_iter_from_last_elem() {
    /* all inlineable
    - 0
      - 1
     */

    let vec = vec![
        es(0, 0, TagStage::Open),
        es(1, 1, TagStage::Open),
        es(1, 1, TagStage::Close),
        es(0, 0, TagStage::Close),
    ];

    let iter = QueueIter::new(vec, None);
    assert_eq!(iter.inlineable, 0..4);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (es(0, 0, TagStage::Open), true),
            (es(1, 1, TagStage::Open), true),
            (es(1, 1, TagStage::Close), true),
            (es(0, 0, TagStage::Close), true),
        ]
    );
}

#[test]
fn non_inlineable_on_last_child_level() {
    /* ! = not inlineable
    - 0
      - 1
      - 2!
     */
    let vec = vec![
        es(0, 0, TagStage::Open),
        es(1, 1, TagStage::Open),
        es(1, 1, TagStage::Close),
    ];

    let last = es(2, 1, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 1..3);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (es(0, 0, TagStage::Open), false),
            (es(1, 1, TagStage::Open), true),
            (es(1, 1, TagStage::Close), true),
            (es(2, 1, TagStage::Open), false),
        ]
    );
}

#[test]
fn non_inlineable_as_top_level_sibling() {
    /* ! = not inlineable
    - 0
      - 1
    - 2
    - 3!
     */
    let vec = vec![
        es(0, 0, TagStage::Open),
        es(1, 1, TagStage::Open),
        es(1, 1, TagStage::Close),
        es(0, 0, TagStage::Close),
        es(2, 0, TagStage::Open),
        es(2, 0, TagStage::Close),
    ];

    let last = es(3, 0, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 0..6);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (es(0, 0, TagStage::Open), true),
            (es(1, 1, TagStage::Open), true),
            (es(1, 1, TagStage::Close), true),
            (es(0, 0, TagStage::Close), true),
            (es(2, 0, TagStage::Open), true),
            (es(2, 0, TagStage::Close), true),
            (es(3, 0, TagStage::Open), false),
        ]
    );
}

#[test]
fn none_inlineable() {
    /* ! = not inlineable
    - 0!
      - 1!
        - 2!
          - 3!
     */
    let vec = vec![
        es(0, 0, TagStage::Open),
        es(1, 1, TagStage::Open),
        es(2, 2, TagStage::Open),
    ];

    let non_inlineable = es(3, 3, TagStage::Open);

    let iter = QueueIter::new(vec, Some(non_inlineable));
    assert_eq!(iter.inlineable, 0..0);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (es(0, 0, TagStage::Open), false),
            (es(1, 1, TagStage::Open), false),
            (es(2, 2, TagStage::Open), false),
            (es(3, 3, TagStage::Open), false),
        ]
    );
}

#[test]
fn none_inlineable_2() {
    /* ! = not inlineable
    - 0!
      - 1!
        - 2
          - 3
        - 4!
          - 5!
     */
    let vec = vec![
        es(0, 0, TagStage::Open),
        es(1, 1, TagStage::Open),
        es(2, 2, TagStage::Open),
        es(3, 3, TagStage::Open),
        es(3, 3, TagStage::Close),
        es(2, 2, TagStage::Close),
        es(4, 2, TagStage::Open),
    ];

    let non_inlineable = es(5, 3, TagStage::Open);

    let iter = QueueIter::new(vec, Some(non_inlineable));
    // assert_eq!(iter.inlineable, 2..6);
    let returned = iter
        .map(|(stage, inline)| (stage.index.as_u32(), stage.depth, stage.stage, inline))
        .collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (0, 0, TagStage::Open, false),
            (1, 1, TagStage::Open, false),
            (2, 2, TagStage::Open, true),
            (3, 3, TagStage::Open, true),
            (3, 3, TagStage::Close, true),
            (2, 2, TagStage::Close, true),
            (4, 2, TagStage::Open, false),
            (5, 3, TagStage::Open, false),
        ]
    );
}

#[test]
fn first_child_not_inlineable() {
    let vec = vec![es(0, 0, TagStage::Open)];

    let last = es(1, 1, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 0..0);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (es(0, 0, TagStage::Open), false),
            (es(1, 1, TagStage::Open), false),
        ]
    );
}
