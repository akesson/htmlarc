use crate::debug;
use crate::html::HtmlTag;
use crate::stores::AttrName;
use std::fmt::Display;

use super::dom::{DomStack, TagName};

/// A tag on the test DOM's stack: a standard [`HtmlTag`], or an extended (custom/unknown) tag
/// held as its rendered name string — full identity, so distinct custom elements never close
/// one another (ADR 0002 §4). Mirrors the real builder's `CursorTag` (which uses a symbol).
#[derive(Clone, PartialEq, Debug)]
pub(crate) enum TestTag {
    Std(HtmlTag),
    Ext(String),
}

pub struct TestElement {
    /// The rendered tag name — a standard tag's spelling, or an extended element's verbatim
    /// name (and `text`/`comment` for the system string nodes).
    tag: String,
    /// `(rendered name, value)` in source order — standard, `data-*`, and unknown alike.
    attrs: Vec<(String, String)>,
    text: Option<String>,
    indentation: usize,
}

impl TestElement {
    fn tag(tag: String, indentation: usize) -> Self {
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
    stack: Vec<TestTag>,
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
    type Tag = TestTag;

    fn make_tag(&mut self, name: TagName<'_>) -> TestTag {
        match name {
            TagName::Std(t) => TestTag::Std(t),
            TagName::Ext(s) => TestTag::Ext(s.to_string()),
        }
    }

    fn kind_of(tag: &TestTag) -> HtmlTag {
        match tag {
            TestTag::Std(t) => *t,
            TestTag::Ext(_) => HtmlTag::extended,
        }
    }

    fn tag_display(&self, tag: &TestTag) -> String {
        match tag {
            TestTag::Std(t) => t.as_str().to_string(),
            TestTag::Ext(s) => s.clone(),
        }
    }

    fn _push_tag(&mut self, tag: TestTag) {
        let name = self.tag_display(&tag);
        debug!("push_tag {name}");
        self.dom.push(TestElement::tag(name, self.stack.len()));
        self.stack.push(tag);
    }

    fn _pop_tag(&mut self) -> Option<TestTag> {
        let tag = self.stack.pop();
        println!("pop_tag '{tag:?}'");
        tag
    }

    fn _last_tag(&self) -> Option<TestTag> {
        self.stack.last().cloned()
    }

    fn stack_info(&self) -> String {
        self.stack
            .iter()
            .map(|t| self.tag_display(t))
            .collect::<Vec<_>>()
            .join(" > ")
    }

    fn add_text_tag(&mut self, tag: HtmlTag, value: &str) {
        debug!("add_text_tag '{tag}'='{value}'");
        self.dom.push(TestElement {
            tag: tag.as_str().to_string(),
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
