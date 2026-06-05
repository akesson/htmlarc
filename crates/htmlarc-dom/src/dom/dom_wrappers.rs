use std::cell::{RefCell, RefMut};
use std::fmt::Debug;

use crate::{
    dom::{DomInner, DomView},
    fmt::HtmlFormat,
};

use crate::html::HtmlElement;

/// Read access to a DOM document, abstracting over *where the bytes live* —
/// owned in memory ([`DomInner`], [`DomOwn`], [`DomRefCell`]) or zero-copy in a
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

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::new(self, 0)
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
pub struct DomOwn {
    pub(crate) dom: DomInner,
}

impl From<DomInner> for DomOwn {
    fn from(dom: DomInner) -> Self {
        Self { dom }
    }
}

impl DomRef for DomOwn {
    fn dom_view(&self) -> DomView<'_> {
        self.dom.view()
    }
}

impl DomRead for DomOwn {
    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        f(self.dom.view())
    }

    fn root(&self) -> HtmlElement<'_, DomOwn> {
        HtmlElement::new(self, 0)
    }

    fn repackage(&self) -> DomInner {
        self.dom.rebuild()
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

    fn root(&self) -> HtmlElement<'_, DomRefCell> {
        HtmlElement::new(self, 0)
    }

    fn repackage(&self) -> DomInner {
        self.dom.borrow().rebuild()
    }
}
