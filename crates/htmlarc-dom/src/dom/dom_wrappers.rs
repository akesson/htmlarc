use std::cell::{RefCell, RefMut};
use std::fmt::Debug;

use crate::{
    dom::{DomInner, DomView, NodeIndex, NodesView},
    fmt::HtmlFormat,
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

impl DomRead for DomInner {
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
