use crate::debug;
use crate::html::{HtmlAttr, HtmlTag};
use std::fmt::Display;

use super::dom::DomStack;

pub struct TestElement {
    tag: HtmlTag,
    attrs: Vec<(HtmlAttr, String)>,
    data_attrs: Vec<(String, String)>,
    text: Option<String>,
    indentation: usize,
}

impl TestElement {
    fn tag(tag: HtmlTag, indentation: usize) -> Self {
        TestElement {
            tag,
            attrs: Vec::new(),
            data_attrs: Vec::new(),
            text: None,
            indentation,
        }
    }
}
impl Display for TestElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}{}{}",
            "  ".repeat(self.indentation),
            self.tag,
            self.attrs
                .iter()
                .map(|(attr, val)| if val.is_empty() {
                    format!(" {attr}")
                } else {
                    format!(" {attr}='{val}'")
                })
                .collect::<Vec<_>>()
                .join(""),
            self.data_attrs
                .iter()
                .map(|(attr, val)| format!(" data-{}='{}'", attr, val))
                .collect::<Vec<_>>()
                .join(""),
            self.text
                .as_ref()
                .map(|t| format!(" '{}'", t.replace('\n', "\\n")))
                .unwrap_or_default()
        )
    }
}

#[derive(Default)]
pub struct TestDom {
    dom: Vec<TestElement>,
    stack: Vec<HtmlTag>,
}

impl TestDom {
    fn push_attr(&mut self, attr: HtmlAttr, value: &str) {
        let current = self.dom.last_mut().unwrap();
        current.attrs.push((attr, value.to_string()))
    }

    fn push_data_attr(&mut self, attr: &str, value: &str) {
        let current = self.dom.last_mut().unwrap();
        current
            .data_attrs
            .push((attr.to_string(), value.to_string()))
    }
}

impl Display for TestDom {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for entry in self.dom.iter() {
            writeln!(f, "{entry}")?;
        }
        Ok(())
    }
}

impl DomStack for TestDom {
    fn _push_tag(&mut self, tag: HtmlTag) {
        debug!("push_tag {tag}");
        self.dom.push(TestElement::tag(tag, self.stack.len()));
        self.stack.push(tag);
    }

    fn _pop_tag(&mut self) -> Option<HtmlTag> {
        let tag = self.stack.pop();
        println!("pop_tag '{tag:?}'");
        tag
    }

    fn _last_tag(&mut self) -> HtmlTag {
        self.stack.last().copied().unwrap_or(HtmlTag::sys_root)
    }

    fn stack_info(&self) -> String {
        self.stack
            .iter()
            .map(HtmlTag::as_str)
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn add_text_tag(&mut self, tag: HtmlTag, value: &str) {
        debug!("add_text_tag '{tag}'='{value}'");
        self.dom.push(TestElement {
            tag,
            attrs: Vec::new(),
            data_attrs: Vec::new(),
            text: Some(value.to_owned()),
            indentation: self.stack.len(),
        })
    }

    fn add_attribute_and_value(&mut self, attribute: HtmlAttr, value: &str) {
        debug!("add_attribute_and_value '{attribute}'='{value}'");
        self.push_attr(attribute, value);
    }

    fn add_data_attribute(&mut self, tag: &str, val: &str) {
        debug!("add_data_attribute '{tag}'='{val}'");
        self.push_data_attr(tag, val);
    }
}
