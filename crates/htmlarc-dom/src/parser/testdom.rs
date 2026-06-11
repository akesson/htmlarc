use crate::debug;
use crate::html::HtmlTag;
use crate::stores::AttrName;
use std::fmt::Display;

use super::dom::DomStack;

pub struct TestElement {
    tag: HtmlTag,
    /// `(rendered name, value)` in source order — standard, `data-*`, and unknown alike.
    attrs: Vec<(String, String)>,
    text: Option<String>,
    indentation: usize,
}

impl TestElement {
    fn tag(tag: HtmlTag, indentation: usize) -> Self {
        TestElement {
            tag,
            attrs: Vec::new(),
            text: None,
            indentation,
        }
    }
}
impl Display for TestElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}{}{}",
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
    fn push_attr(&mut self, name: String, value: &str) {
        let current = self.dom.last_mut().unwrap();
        current.attrs.push((name, value.to_string()))
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
            text: Some(value.to_owned()),
            indentation: self.stack.len(),
        })
    }

    fn add_attribute(&mut self, name: AttrName<'_>, value: &str) {
        debug!("add_attribute '{name}'='{value}'");
        self.push_attr(name.to_string(), value);
    }
}
