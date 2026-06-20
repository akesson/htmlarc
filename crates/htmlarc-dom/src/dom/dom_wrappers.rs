use std::cell::{RefCell, RefMut};
use std::fmt::Debug;

use crate::{
    dom::{DomInner, DomView, NodeIndex, NodesView},
    fmt::HtmlFormat,
    iters::{DomIterator, ElementIter, LinearSweep},
};

use crate::html::HtmlElement;

/// Read access to a DOM document, abstracting over *where the bytes live* —
/// owned in memory ([`DomInner`], [`DomRefCell`]) or zero-copy in a
/// memory-mapped rkyv archive (`ArchivedDomInner`). All query code (iterators, CSS,
/// the formatter, [`HtmlElement`]) goes through [`DomView`] via [`Self::with_view`],
/// so it never needs to know which.
pub trait DomRead
where
    Self: Sized + Debug,
{
    /// `true` ⇒ the backing cannot change while a walk over it is live, so iterators may skip
    /// the per-step mutation-recovery in [`VisitedStack::last_updated`](crate::iters). Every
    /// backing is immutable during a `&self` read except [`DomRefCell`], which leaves this
    /// `false` (the conservative default) so mutate-while-iterating stays correct.
    const IS_IMMUTABLE: bool = false;

    /// The iterator type [`HtmlElement::forwards`](crate::html::HtmlElement::forwards) yields for
    /// this backing: the layout-exploiting [`LinearSweep`] for contiguous backings, the
    /// tree-walking [`ElementIter`] for the mutable `DomRefCell`. Per-backing dispatch through an
    /// associated type means the public iteration API is uniform while each backing gets the
    /// fastest correct implementation — no runtime branch, no `dyn`.
    type Forward<'a>: DomIterator<'a, Self>
    where
        Self: 'a;
    /// The iterator type [`HtmlElement::descendants`](crate::html::HtmlElement::descendants) yields
    /// for this backing — see [`Forward`](Self::Forward).
    type Descendants<'a>: DomIterator<'a, Self>
    where
        Self: 'a;

    /// Build the document-order forward walk from `start` (its tail to end of document). Called by
    /// `HtmlElement::forwards`; the choice of [`LinearSweep`] vs [`ElementIter`] is the backing's.
    fn forward_from(&self, start: NodeIndex) -> Self::Forward<'_>;

    /// Build the descendant (subtree) walk from `start`. Called by `HtmlElement::descendants`.
    fn descendants_from(&self, start: NodeIndex) -> Self::Descendants<'_>;

    /// Run `f` with a borrowed [`DomView`]. The closure form (rather than returning
    /// the view) is what lets [`DomRefCell`] scope its `RefCell` borrow guard.
    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R;

    /// Run `f` with just the topology sub-view ([`NodesView`]). Backings override this to build
    /// *only* the nodes view (a width enum + a borrowed slice), skipping the five other sub-views
    /// that [`Self::with_view`] assembles. The hot traversal path — sibling/child walks and the
    /// tag reads that drive text/comment skipping — needs topology alone, so this avoids rebuilding
    /// the full [`DomView`] ~2-3× per element. The default delegates to [`with_view`](Self::with_view)
    /// so a backing is correct without overriding, just unoptimized.
    fn with_nodes<F: FnOnce(NodesView<'_>) -> R, R>(&self, f: F) -> R {
        self.with_view(|view| f(view.nodes))
    }

    fn root(&self) -> HtmlElement<'_, Self>;

    /// Materialise an owned, compacted [`DomInner`]. Owned backings rebuild in place;
    /// the archived backing deserializes out of the archive.
    fn repackage(&self) -> DomInner;

    fn to_html(&self, fmt: HtmlFormat) -> String {
        let index = self.root().index();
        self.with_view(|view| fmt.to_html(view, index))
    }
}

/// A [`DomRead`] that can additionally hand out a [`DomView`] bound to the *full*
/// `&self` lifetime (not just a closure scope). Implemented by every backing whose
/// bytes outlive a single call — i.e. all except [`DomRefCell`].
pub trait DomRef: DomRead {
    fn dom_view(&self) -> DomView<'_>;
}

/// Marker promising that a backing's node blob is contiguous DFS pre-order with no dead slots, so a
/// node's `NodeIndex` is its document-order position and forward/descendant walks reduce to integer
/// index ranges ([`LinearSweep`]). This is the bound that gates `LinearSweep`'s constructors, so a
/// backing opts into the fast linear walk by implementing it.
///
/// Strictly stronger than [`DomRead::IS_IMMUTABLE`]: a [`DomInner`] is immutable yet may carry dead
/// slots before a rebuild (see `rebuilder`), so it is contiguous *by contract* — every way to
/// obtain a bare `DomInner` (parse, rebuild, deserialize) yields a contiguous blob, and the only
/// dead-slot state lives behind a [`DomRefCell`] (which is *not* `ContiguousDfs`, so it tree-walks)
/// or is consumed by `rebuild` itself (which tree-walks explicitly via `*_walk`).
///
/// # Contract
///
/// Implement this only for a backing whose blob is *always* contiguous when read (mmap archives,
/// freshly parsed/rebuilt owned docs). Implementing it for a backing that can expose dead slots
/// makes a linear walk emit them — a logic error, debug-asserted at iterator construction. Query
/// users never name this trait; only backing implementors do.
pub trait ContiguousDfs: DomRead {}

impl DomRead for DomInner {
    const IS_IMMUTABLE: bool = true;

    type Forward<'a> = LinearSweep<'a, DomInner>;
    type Descendants<'a> = LinearSweep<'a, DomInner>;

    fn forward_from(&self, start: NodeIndex) -> Self::Forward<'_> {
        LinearSweep::forwards_at(self, start)
    }

    fn descendants_from(&self, start: NodeIndex) -> Self::Descendants<'_> {
        LinearSweep::descendants_at(self, start)
    }

    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.view())
    }

    fn with_nodes<F: FnOnce(NodesView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.nodes.view())
    }

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::new(self, NodeIndex::ROOT)
    }

    fn repackage(&self) -> DomInner {
        self.rebuild()
    }
}
impl DomRef for DomInner {
    fn dom_view(&self) -> DomView<'_> {
        self.view()
    }
}
impl ContiguousDfs for DomInner {}

#[derive(Debug)]
pub struct DomRefCell {
    pub(crate) dom: RefCell<DomInner>,
}

impl DomRefCell {
    pub fn new(inner: DomInner) -> Self {
        Self {
            dom: RefCell::new(inner),
        }
    }

    pub fn with_mut<R, F: Fn(&mut DomInner) -> R>(&self, f: F) -> R {
        f(&mut self.dom.borrow_mut())
    }
    pub fn mut_handle(&self) -> RefMut<'_, DomInner> {
        self.dom.borrow_mut()
    }
}

impl DomRead for DomRefCell {
    // The mutable backing keeps the tree-walking iterator: it skips dead slots and recovers from
    // mutate-while-iterating. It is deliberately NOT `ContiguousDfs`, so `LinearSweep` can never be
    // built for it.
    type Forward<'a> = ElementIter<'a, DomRefCell>;
    type Descendants<'a> = ElementIter<'a, DomRefCell>;

    fn forward_from(&self, start: NodeIndex) -> Self::Forward<'_> {
        ElementIter::forwards_at(self, start)
    }

    fn descendants_from(&self, start: NodeIndex) -> Self::Descendants<'_> {
        ElementIter::descendants_at(self, start)
    }

    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.dom.borrow().view())
    }

    fn with_nodes<F: FnOnce(NodesView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.dom.borrow().nodes.view())
    }

    fn root(&self) -> HtmlElement<'_, DomRefCell> {
        HtmlElement::new(self, NodeIndex::ROOT)
    }

    fn repackage(&self) -> DomInner {
        self.dom.borrow().rebuild()
    }
}
