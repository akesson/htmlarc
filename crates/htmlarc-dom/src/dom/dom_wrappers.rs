use std::cell::{RefCell, RefMut};
use std::fmt::Debug;

use crate::{dom::DomInner, fmt::HtmlFormat};

use crate::html::HtmlElement;

pub trait DomRead
where
    Self: Sized + Debug,
{
    fn with_dom<F: FnOnce(&DomInner) -> R, R>(&self, f: F) -> R;

    fn root(&self) -> HtmlElement<'_, Self>;

    fn to_html(&self, fmt: HtmlFormat) -> String {
        let index = self.root().index();
        self.with_dom(|dom| fmt.to_html(dom, index))
    }

    fn repackage(&self) -> DomInner {
        self.with_dom(|dom| dom.rebuild())
    }
}

pub trait DomRef: DomRead {
    fn as_ref(&self) -> &DomInner;
}

impl DomRead for DomInner {
    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::new(self, 0)
    }

    fn with_dom<F: FnOnce(&DomInner) -> R, R>(&self, f: F) -> R {
        f(self)
    }
}
impl DomRef for DomInner {
    fn as_ref(&self) -> &DomInner {
        self
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
    fn as_ref(&self) -> &DomInner {
        &self.dom
    }
}

impl DomRead for DomOwn {
    fn with_dom<F: FnOnce(&DomInner) -> R, R>(&self, f: F) -> R {
        f(&self.dom)
    }

    fn root(&self) -> HtmlElement<'_, DomOwn> {
        HtmlElement::new(self, 0)
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
    fn with_dom<F: FnOnce(&DomInner) -> R, R>(&self, f: F) -> R {
        f(&self.dom.borrow())
    }

    fn root(&self) -> HtmlElement<'_, DomRefCell> {
        HtmlElement::new(self, 0)
    }
}
