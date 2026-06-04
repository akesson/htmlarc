use crate::probe::format::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ElementAttribute<'dom> {
    Class(Class<'dom>),
    Attribute(Attribute<'dom>),
    DataAttribute(DataAttribute<'dom>),
    Text(String),
}

impl ElementAttribute<'_> {
    pub fn format(&self, style: &ElementStyle) -> String {
        match style {
            ElementStyle::HtmlFmt => match self {
                ElementAttribute::Class(class) => class.to_string(),
                ElementAttribute::Attribute(attr) => format!("{}=\'{}\'", attr.tag, attr.val),
                ElementAttribute::DataAttribute(data) => {
                    format!("data-{}=\'{}\'", data.tag, data.val)
                }
                ElementAttribute::Text(text) => format!("text=\'{}\'", text),
            },
            ElementStyle::CssFmt => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute(attribute) => {
                    if attribute.tag == HtmlAttr::id {
                        format!("#{}", attribute.val)
                    } else {
                        format!("[{}=\'{}\']", attribute.tag, attribute.val)
                    }
                }
                ElementAttribute::DataAttribute(data_attribute) => {
                    format!("[data-{}=\'{}\']", data_attribute.tag, data_attribute.val)
                }
                ElementAttribute::Text(text) => format!("[text=\'{}\']", text),
            },
            ElementStyle::CssTerse => match self {
                ElementAttribute::Class(class) => format!(".{}", class),
                ElementAttribute::Attribute(attribute) => {
                    if attribute.tag == HtmlAttr::id {
                        format!("#{}", attribute.val)
                    } else {
                        format!("[\'{}\']", attribute.val)
                    }
                }
                ElementAttribute::DataAttribute(data_attribute) => {
                    format!("[\'{}\']", data_attribute.val)
                }
                ElementAttribute::Text(text) => format!("[\'{}\']", text),
            },
        }
    }
}
