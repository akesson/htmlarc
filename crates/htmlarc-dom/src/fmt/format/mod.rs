mod common;
mod pretty;
mod raw;

use crate::dom::{DomView, NodeIndex};
use pretty::PrettyFormat;
use raw::RawFormat;

#[derive(Debug, Clone, Copy)]
pub enum HtmlFormat {
    Raw,
    Pretty,
}

impl HtmlFormat {
    pub fn raw_else_pretty(raw: bool) -> Self {
        if raw { Self::Raw } else { Self::Pretty }
    }
    pub fn to_html(&self, dom: DomView<'_>, index: NodeIndex) -> String {
        match self {
            Self::Raw => RawFormat::new(dom).html(index),
            Self::Pretty => PrettyFormat::new(dom).html(index),
        }
    }
}
