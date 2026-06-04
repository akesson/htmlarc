use super::tag_iter::{ElementStage, TagStage};
use std::ops::Range;

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
        ElementStage::new(0, 0, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Close),
        ElementStage::new(0, 0, TagStage::Close),
    ];

    let iter = QueueIter::new(vec, None);
    assert_eq!(iter.inlineable, 0..4);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (ElementStage::new(0, 0, TagStage::Open), true),
            (ElementStage::new(1, 1, TagStage::Open), true),
            (ElementStage::new(1, 1, TagStage::Close), true),
            (ElementStage::new(0, 0, TagStage::Close), true),
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
        ElementStage::new(0, 0, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Close),
    ];

    let last = ElementStage::new(2, 1, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 1..3);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (ElementStage::new(0, 0, TagStage::Open), false),
            (ElementStage::new(1, 1, TagStage::Open), true),
            (ElementStage::new(1, 1, TagStage::Close), true),
            (ElementStage::new(2, 1, TagStage::Open), false),
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
        ElementStage::new(0, 0, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Close),
        ElementStage::new(0, 0, TagStage::Close),
        ElementStage::new(2, 0, TagStage::Open),
        ElementStage::new(2, 0, TagStage::Close),
    ];

    let last = ElementStage::new(3, 0, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 0..6);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (ElementStage::new(0, 0, TagStage::Open), true),
            (ElementStage::new(1, 1, TagStage::Open), true),
            (ElementStage::new(1, 1, TagStage::Close), true),
            (ElementStage::new(0, 0, TagStage::Close), true),
            (ElementStage::new(2, 0, TagStage::Open), true),
            (ElementStage::new(2, 0, TagStage::Close), true),
            (ElementStage::new(3, 0, TagStage::Open), false),
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
        ElementStage::new(0, 0, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Open),
        ElementStage::new(2, 2, TagStage::Open),
    ];

    let non_inlineable = ElementStage::new(3, 3, TagStage::Open);

    let iter = QueueIter::new(vec, Some(non_inlineable));
    assert_eq!(iter.inlineable, 0..0);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (ElementStage::new(0, 0, TagStage::Open), false),
            (ElementStage::new(1, 1, TagStage::Open), false),
            (ElementStage::new(2, 2, TagStage::Open), false),
            (ElementStage::new(3, 3, TagStage::Open), false),
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
        ElementStage::new(0, 0, TagStage::Open),
        ElementStage::new(1, 1, TagStage::Open),
        ElementStage::new(2, 2, TagStage::Open),
        ElementStage::new(3, 3, TagStage::Open),
        ElementStage::new(3, 3, TagStage::Close),
        ElementStage::new(2, 2, TagStage::Close),
        ElementStage::new(4, 2, TagStage::Open),
    ];

    let non_inlineable = ElementStage::new(5, 3, TagStage::Open);

    let iter = QueueIter::new(vec, Some(non_inlineable));
    // assert_eq!(iter.inlineable, 2..6);
    let returned = iter
        .map(|(stage, inline)| (stage.index, stage.depth, stage.stage, inline))
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
    let vec = vec![ElementStage::new(0, 0, TagStage::Open)];

    let last = ElementStage::new(1, 1, TagStage::Open);
    let iter = QueueIter::new(vec, Some(last));
    assert_eq!(iter.inlineable, 0..0);
    let returned = iter.collect::<Vec<_>>();

    assert_eq!(
        returned,
        vec![
            (ElementStage::new(0, 0, TagStage::Open), false),
            (ElementStage::new(1, 1, TagStage::Open), false),
        ]
    );
}
