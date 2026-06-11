use crate::probe::format::*;
use htmlarc_dom::prelude::AttrName;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementAttribute<'dom> {
    Class(Class<'dom>),
    Attribute(Attribute<'dom>),
    Text(String),
}

impl ElementAttribute<'_> {
    pub fn format(&self, style: &ElementStyle) -> String {
        match style {
            ElementStyle::HtmlFmt => match self {
                ElementAttribute::Class(class) => class.to_string(),
                ElementAttribute::Attribute(attr) => format!("{}=\'{}\'", attr.name, attr.val),
                ElementAttribute::Text(text) => format!("text=\'{}\'", text),
            },
            ElementStyle::CssFmt => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute(attribute) => {
                    if attribute.name == AttrName::Std(HtmlAttr::id) {
                        format!("#{}", attribute.val)
                    } else {
                        format!("[{}=\'{}\']", attribute.name, attribute.val)
                    }
                }
                ElementAttribute::Text(text) => format!("[text=\'{}\']", text),
            },
            ElementStyle::CssTerse => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute(attribute) => {
                    if attribute.name == AttrName::Std(HtmlAttr::id) {
                        format!("#{}", attribute.val)
                    } else {
                        format!("[\'{}\']", attribute.val)
                    }
                }
                ElementAttribute::Text(text) => format!("[\'{}\']", text),
            },
        }
    }
}
