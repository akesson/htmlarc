use crate::probe::format::*;

/// Fully-owned attribute representation (no borrow into the document), so a probe result is
/// independent of the source document's lifetime. `is_id` is captured at construction so the
/// CSS formatters can still render `#id` without holding an `AttrName` that borrows the document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementAttribute {
    Class(String),
    Attribute {
        name: String,
        is_id: bool,
        val: String,
    },
    Text(String),
}

impl ElementAttribute {
    pub fn format(&self, style: &ElementStyle) -> String {
        match style {
            ElementStyle::HtmlFmt => match self {
                ElementAttribute::Class(class) => class.clone(),
                ElementAttribute::Attribute { name, val, .. } => format!("{name}=\'{val}\'"),
                ElementAttribute::Text(text) => format!("text=\'{}\'", text),
            },
            ElementStyle::CssFmt => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute { name, is_id, val } => {
                    if *is_id {
                        format!("#{}", val)
                    } else {
                        format!("[{}=\'{}\']", name, val)
                    }
                }
                ElementAttribute::Text(text) => format!("[text=\'{}\']", text),
            },
            ElementStyle::CssTerse => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute { is_id, val, .. } => {
                    if *is_id {
                        format!("#{}", val)
                    } else {
                        format!("[\'{}\']", val)
                    }
                }
                ElementAttribute::Text(text) => format!("[\'{}\']", text),
            },
        }
    }
}
