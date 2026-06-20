use std::cell::Cell;

use crate::{
    dom::{ContiguousDfs, DomRead, NodeIndex, NodesView},
    html::{HtmlElement, HtmlTag},
};

use super::DomIterator;

/// A document-order walk that exploits the on-disk node layout instead of walking the tree.
///
/// htmlarc serializes nodes in **DFS pre-order**, so a node's `NodeIndex` *is* its document-order
/// position, and on a [`ContiguousDfs`] backing (mmap archive or freshly parsed/rebuilt `DomInner`)
/// the blob is contiguous with no dead slots. Under that invariant the two document-order walks the
/// query layer needs reduce to integer index ranges:
/// - `forwards(i)` — every node after `i` to the end of the document — is `[i+1, len)`.
/// - `descendants(i)` — `i`'s subtree — is `[i+1, subtree_end(i))`.
///
/// So `LinearSweep` is a half-open ascending index range `[front, back)` walked with a `u32`
/// counter that reads **no topology link bytes at all**, replacing the `VisitedStack` +
/// `find_next` + per-step parent/sibling/child reads that dominate the tree-walk (62–72% of select
/// self-time per the profiling notes). It is the per-backing default behind
/// [`HtmlElement::forwards`]/[`descendants`] for immutable backings; the mutable `DomRefCell` keeps
/// the tree-walking [`ElementIter`](super::ElementIter) (mutation-safe, dead-slot-skipping).
///
/// Being a flat range, it is the only document-order walk that can implement
/// [`DoubleEndedIterator`] — `.rev()` / `next_back()` yield the range high→low. (`std::iter::Rev`
/// is not a [`DomIterator`], so the char/select conveniences are unreachable on the reversed form,
/// which is why there is no dedicated reverse constructor: reverse-toward-document-start stays
/// [`RevElementIter`](super::RevElementIter).)
///
/// [`HtmlElement::forwards`]: crate::html::HtmlElement::forwards
/// [`descendants`]: crate::html::HtmlElement::descendants
pub struct LinearSweep<'dom, Dom> {
    dom: &'dom Dom,
    /// Next index to yield from the front (inclusive low bound).
    front: Cell<u32>,
    /// One past the last index to yield (exclusive high bound).
    back: Cell<u32>,
    include_comment: bool,
    include_text: bool,
}

impl<'dom, Dom> Clone for LinearSweep<'dom, Dom> {
    fn clone(&self) -> Self {
        Self {
            dom: self.dom,
            front: self.front.clone(),
            back: self.back.clone(),
            include_comment: self.include_comment,
            include_text: self.include_text,
        }
    }
}

/// Debug-only guard on the [`ContiguousDfs`] contract at `LinearSweep` construction (compiled out
/// in release). On normal-size blobs it runs the full [`NodesView::is_contiguous_dfs`] scan, which
/// catches a dead slot *anywhere* in the range — the tripwire for any future internal code that
/// builds a linear walk over a mid-mutation `DomInner` instead of using `forwards_walk`/
/// `descendants_walk`. Above `FULL_SCAN_LIMIT` it falls back to a front-boundary spot-check so a
/// `:has` query (one construction per candidate) can't go quadratic on a large document; every real
/// fixture is far below the cap, so they get the full check.
#[inline]
#[cfg_attr(not(debug_assertions), allow(unused_variables))]
fn debug_check_contiguous(nodes: NodesView, front: u32, back: u32) {
    debug_assert!(front <= back, "LinearSweep front {front} past back {back}");
    #[cfg(debug_assertions)]
    {
        const FULL_SCAN_LIMIT: usize = 8192;
        if nodes.len() <= FULL_SCAN_LIMIT {
            assert!(
                nodes.is_contiguous_dfs(),
                "LinearSweep over a non-contiguous blob (dead slot present) — a dirty DomInner \
                 must iterate via forwards_walk/descendants_walk, not the dispatched forwards/\
                 descendants"
            );
        } else if front < back && front > 0 {
            assert!(
                matches!(nodes.parent_index(NodeIndex::new(front)), Some(p) if p.as_u32() < front),
                "LinearSweep over a non-contiguous blob (dead slot at index {front}?)"
            );
        }
    }
}

impl<'dom, Dom: ContiguousDfs> LinearSweep<'dom, Dom> {
    /// Every node after the node at `start` to the end of the document — the linear form of
    /// [`ElementIter::forwards_at`](super::ElementIter::forwards_at). `pub` so out-of-crate backings
    /// (e.g. the archive's mmap `Doc` handle) can return it from `DomRead::forward_from`.
    pub fn forwards_at(dom: &'dom Dom, start: NodeIndex) -> Self {
        let (front, back) = dom.with_nodes(|nodes| {
            let len = nodes.len() as u32;
            let front = (start.as_u32() + 1).min(len);
            debug_check_contiguous(nodes, front, len);
            (front, len)
        });
        Self::range(dom, front, back)
    }

    /// The subtree rooted at `start` (excluding `start`) — the linear form of
    /// [`ElementIter::descendants_at`](super::ElementIter::descendants_at). `pub`; see
    /// [`forwards_at`](Self::forwards_at).
    pub fn descendants_at(dom: &'dom Dom, start: NodeIndex) -> Self {
        let (front, back) = dom.with_nodes(|nodes| {
            let len = nodes.len() as u32;
            let front = (start.as_u32() + 1).min(len);
            let back = nodes.subtree_end(start) as u32;
            debug_check_contiguous(nodes, front, back);
            (front, back)
        });
        Self::range(dom, front, back)
    }

    fn range(dom: &'dom Dom, front: u32, back: u32) -> Self {
        Self {
            dom,
            front: Cell::new(front),
            back: Cell::new(back),
            include_comment: false,
            include_text: false,
        }
    }
}

impl<'dom, Dom: DomRead> LinearSweep<'dom, Dom> {
    /// Whether a node with `tag` passes the current text/comment filter — shared by the front
    /// ([`DomIterator::next_element`]) and back ([`DoubleEndedIterator::next_back`]) filter loops.
    fn admit(&self, tag: HtmlTag) -> bool {
        match tag {
            HtmlTag::sys_text => self.include_text,
            HtmlTag::sys_comment => self.include_comment,
            _ => true,
        }
    }

    /// Pop and admit nodes from the back of the range. Mirrors the trait's front-side
    /// `next_element`, but the trait default only consumes the front, so the back path is explicit.
    fn next_back_element(&self) -> Option<HtmlElement<'dom, Dom>> {
        loop {
            let (front, back) = (self.front.get(), self.back.get());
            if front >= back {
                return None;
            }
            let index = back - 1;
            self.back.set(index);
            let el = HtmlElement::new(self.dom, NodeIndex::new(index));
            if self.admit(el.tag()) {
                return Some(el);
            }
        }
    }
}

impl<'dom, Dom: DomRead> Iterator for LinearSweep<'dom, Dom> {
    type Item = HtmlElement<'dom, Dom>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_element()
    }
}

impl<'dom, Dom: DomRead> DoubleEndedIterator for LinearSweep<'dom, Dom> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.next_back_element()
    }
}

impl<'dom, Dom: DomRead> DomIterator<'dom, Dom> for LinearSweep<'dom, Dom> {
    fn dom(&self) -> &'dom Dom {
        self.dom
    }

    /// The whole point: no `with_nodes`, no link byte-reads, no stack — just bump the front cursor.
    /// Text/comment filtering stays in [`DomIterator::next_element`] (and `MatchIter` does its own
    /// `sys_text` skip), so the yielded element stream is identical to the tree-walk's.
    fn next_index(&self) -> Option<NodeIndex> {
        let front = self.front.get();
        if front < self.back.get() {
            self.front.set(front + 1);
            Some(NodeIndex::new(front))
        } else {
            None
        }
    }

    fn set_include_comment(mut self) -> Self {
        self.include_comment = true;
        self
    }

    fn include_comment(&self) -> bool {
        self.include_comment
    }

    fn set_include_text(mut self) -> Self {
        self.include_text = true;
        self
    }

    fn include_text(&self) -> bool {
        self.include_text
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use crate::css::parse_css;
    use crate::dom::{DomRead, NodeIndex};
    use crate::html::{HtmlDoc, HtmlElement, HtmlTag};
    use crate::iters::DomIterator;

    // Same shape as the ElementIter/RevElementIter fixtures: nested elements, text, comments.
    const HTML: &str = r#"
<header>
    <h1></h1>
</header>
<main>
    <section>
        <p></p><!-- comment -->
        <article></article>
        <div>
            <span></span>
            <h2></h2>
            <!-- comment -->
            <p></p>
        </div>
        <a></a><!-- comment -->
        <h4></h4>
    </section>
</main>
"#;

    /// On the immutable `DomInner`, `forwards()`/`descendants()` ARE the linear sweep. They must
    /// yield the exact same element-index stream as the explicit tree-walk (`*_walk`), in every
    /// filter mode, from the root and from a mid-tree node.
    #[test]
    fn linear_matches_tree_walk() {
        let dom = HtmlDoc::parse(HTML).unwrap().dom();
        let div = dom
            .root()
            .forwards()
            .find(|e| e.tag() == HtmlTag::div)
            .unwrap();

        for start in [dom.root(), div] {
            // forwards, all four include modes
            for (text, comment) in [(false, false), (true, false), (false, true), (true, true)] {
                let mut linear = start.forwards();
                let mut walk = start.forwards_walk();
                if text {
                    linear = linear.set_include_text();
                    walk = walk.set_include_text();
                }
                if comment {
                    linear = linear.set_include_comment();
                    walk = walk.set_include_comment();
                }
                let l: Vec<u32> = linear.map(|e| e.index().as_u32()).collect();
                let w: Vec<u32> = walk.map(|e| e.index().as_u32()).collect();
                assert_eq!(l, w, "forwards text={text} comment={comment}");

                // descendants, same modes
                let mut linear = start.descendants();
                let mut walk = start.descendants_walk();
                if text {
                    linear = linear.set_include_text();
                    walk = walk.set_include_text();
                }
                if comment {
                    linear = linear.set_include_comment();
                    walk = walk.set_include_comment();
                }
                let l: Vec<u32> = linear.map(|e| e.index().as_u32()).collect();
                let w: Vec<u32> = walk.map(|e| e.index().as_u32()).collect();
                assert_eq!(l, w, "descendants text={text} comment={comment}");
            }
        }
    }

    /// `subtree_end` is the descendant range's exclusive upper bound: a contiguous slice.
    #[test]
    fn descendants_is_a_contiguous_range() {
        let dom = HtmlDoc::parse(HTML).unwrap().dom();
        let div = dom
            .root()
            .forwards()
            .find(|e| e.tag() == HtmlTag::div)
            .unwrap();

        let indices: Vec<u32> = div
            .descendants()
            .set_include_text()
            .set_include_comment()
            .map(|e| e.index().as_u32())
            .collect();
        // contiguous and strictly after the start
        assert!(indices.windows(2).all(|w| w[1] == w[0] + 1));
        assert_eq!(indices.first().copied(), Some(div.index().as_u32() + 1));
    }

    /// `select` (now linear on the immutable backing) returns the same matches as the tree-walk.
    #[test]
    fn select_matches_tree_walk() {
        let dom = HtmlDoc::parse(HTML).unwrap().dom();
        let root = dom.root();
        for css in ["p", "div p", "section > div", "h2, h4", "section span"] {
            let linear: Vec<u32> = root
                .select(parse_css(css).unwrap())
                .map(|e| e.index().as_u32())
                .collect();
            let walk: Vec<u32> = root
                .select_walk(parse_css(css).unwrap())
                .map(|e| e.index().as_u32())
                .collect();
            assert_eq!(linear, walk, "query: {css}");
        }
    }

    /// `text_content` (descendants + chars, linear here) equals the tree-walk's.
    #[test]
    fn text_content_matches_tree_walk() {
        let dom = HtmlDoc::parse("<body>ab<div>cd<span>ef</span></div>gh</body>")
            .unwrap()
            .dom();
        let body = dom
            .root()
            .forwards()
            .find(|e| e.tag() == HtmlTag::body)
            .unwrap();
        let linear: String = body.descendants().text_chars().collect();
        let walk: String = body.descendants_walk().text_chars().collect();
        assert_eq!(linear, walk);
        assert_eq!(linear, "abcdefgh");
    }

    /// `DoubleEndedIterator`: `.rev()` reverses the forward stream, and interleaving
    /// `next()`/`next_back()` covers the range exactly once with no overlap.
    #[test]
    fn double_ended() {
        let dom = HtmlDoc::parse(HTML).unwrap().dom();
        let root = dom.root();

        let fwd: Vec<u32> = root.forwards().map(|e| e.index().as_u32()).collect();
        let revved: Vec<u32> = root.forwards().rev().map(|e| e.index().as_u32()).collect();
        let mut expect_rev = fwd.clone();
        expect_rev.reverse();
        assert_eq!(revved, expect_rev);

        // meet in the middle: pull alternately from both ends, must equal the full set once.
        let mut it = root.forwards();
        let mut got = VecDeque::new();
        let mut from_front = true;
        loop {
            let next = if from_front {
                it.next()
            } else {
                it.next_back()
            };
            match next {
                Some(e) => {
                    if from_front {
                        got.push_back(e.index().as_u32());
                    } else {
                        got.push_front(e.index().as_u32());
                    }
                }
                None => break,
            }
            from_front = !from_front;
        }
        let mut got: Vec<u32> = got.into();
        got.sort_unstable();
        let mut all = fwd.clone();
        all.sort_unstable();
        assert_eq!(got, all);
    }

    /// `exactly` is now a `DomIterator` trait method, so it works on the linear sweep too.
    #[test]
    fn exactly_via_trait() {
        let dom = HtmlDoc::parse("<body><div>hi</div></body>").unwrap().dom();
        let linear: Vec<String> = dom
            .root()
            .forwards()
            .set_include_text()
            .exactly(2..=3)
            .map(|r| {
                r.map(|e| e.tag().to_string())
                    .unwrap_or_else(|e| e.to_string())
            })
            .collect();
        let walk: Vec<String> = dom
            .root()
            .forwards_walk()
            .set_include_text()
            .exactly(2..=3)
            .map(|r| {
                r.map(|e| e.tag().to_string())
                    .unwrap_or_else(|e| e.to_string())
            })
            .collect();
        assert_eq!(linear, walk);
    }

    /// The contiguity invariant `IS_IMMUTABLE` does NOT guarantee: a removed-but-not-rebuilt
    /// `DomInner` has a dead slot (`parent == None`), so `is_contiguous_dfs()` is false. This is
    /// why the linear sweep is gated on the blob, not on immutability.
    #[test]
    fn dead_slot_fails_contiguity_check() {
        let dom = HtmlDoc::parse("<body><div>x</div><span>y</span></body>")
            .unwrap()
            .dom_ref_cell();
        // remove the <div> subtree without rebuilding → leaves a dead slot in the blob.
        let div = dom
            .root()
            .forwards()
            .find(|e| e.tag() == HtmlTag::div)
            .unwrap();
        div.remove();
        let contiguous = dom.with_mut(|inner| inner.is_contiguous_dfs());
        assert!(
            !contiguous,
            "a removed-but-not-rebuilt DomInner must fail the check"
        );
    }

    /// The construction guard fires: building a `LinearSweep` over a dirty (dead-slotted) `DomInner`
    /// panics in debug builds. This is the tripwire for future internal code that forgets `*_walk`
    /// on a mid-mutation blob. (Debug-only: the guard is compiled out of release.)
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "non-contiguous blob")]
    fn linear_sweep_over_dirty_dominner_panics() {
        let dom = HtmlDoc::parse("<body><div>x</div><span>y</span></body>")
            .unwrap()
            .dom_ref_cell();
        let div = dom
            .root()
            .forwards()
            .find(|e| e.tag() == HtmlTag::div)
            .unwrap();
        div.remove(); // leaves a dead slot
        dom.with_mut(|inner| {
            // Forcing the dispatched (linear) walk over the dirty bare &DomInner is the bug the
            // guard catches — constructing it runs `debug_check_contiguous`, which panics here.
            let _ = HtmlElement::new(&*inner, NodeIndex::ROOT).forwards();
        });
    }
}
