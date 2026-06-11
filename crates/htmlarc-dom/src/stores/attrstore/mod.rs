//! ADR 0002 §3 — the unified per-document attribute store.
//!
//! One [`AttrStore`] holds every attribute of a document: standard (`HtmlAttr`), `data-*`,
//! and otherwise-unknown names alike. An attribute is a `(NameSym, ValueRef)` pair; the
//! values live in the store's own [`SymbolTable`](crate::stores::SymbolTable) and the
//! extended names share the document's [`symbols`](crate::dom::DomInner) table with class
//! tokens. Per-element attribute lists are contiguous runs of entry ids in a
//! [`RunVec`](crate::stores::RunVec) — the same arena machinery class lists use.

#[cfg(test)]
mod tests;

mod builder;
mod rebuilder;
mod store;

use std::fmt::Display;

use crate::html::HtmlAttr;

pub(crate) use builder::AttrStoreBuilder;
pub use store::AttrStore;
pub(crate) use store::AttrStoreView;

/// A `NameSym` below this is a standard [`HtmlAttr`] (stored as its `u8` repr); at or above
/// it the name is an extended symbol — `(sym + NAME_EXT_BASE)` indexes the document symbol
/// table (ADR 0002 §2). The bias fits: the largest local sym is `LOCAL_CAP - 1`, so the
/// largest extended `NameSym` is `LOCAL_CAP - 1 + 256 = SHARED_BASE - 1`, still below the
/// reserved shared range and the `0xFFFF` sentinel (asserted in `store.rs`).
pub(crate) const NAME_EXT_BASE: u16 = 256;

/// An attribute name: a known [`HtmlAttr`], or an extended name (any `data-*` or otherwise
/// unrecognised attribute) borrowed from the document symbol table or a selector string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AttrName<'a> {
    Std(HtmlAttr),
    Ext(&'a str),
}

impl Display for AttrName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttrName::Std(attr) => write!(f, "{attr}"),
            AttrName::Ext(name) => write!(f, "{name}"),
        }
    }
}

/// A single attribute: its name and (entity-decoded) value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Attribute<'a> {
    pub name: AttrName<'a>,
    pub val: &'a str,
}

impl Display for Attribute<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.val.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}=\"{}\"", self.name, self.val)
        }
    }
}
