use std::fmt::Display;

use crate::dom::NodeIndex;
use crate::html::{HtmlAttr, HtmlTag};

use crate::{HtmlParseError, HtmlParseResult};

pub trait DomStack {
    fn push_tag(&mut self, tag: HtmlTag) {
        let current = self._last_tag();
        if current.auto_close_when_next(tag) {
            self._pop_tag();
        }
        self._push_tag(tag)
    }

    /// Only for internal use
    fn _pop_tag(&mut self) -> Option<HtmlTag>;

    /// Only for internal use
    fn _last_tag(&mut self) -> HtmlTag;

    fn _push_tag(&mut self, tag: HtmlTag);

    fn stack_info(&self) -> String;

    /// Only for adding comments and text
    fn add_text_tag(&mut self, tag: HtmlTag, value: &str);

    fn add_attribute_and_value(&mut self, attribute: HtmlAttr, value: &str);

    fn add_data_attribute(&mut self, attribute: &str, value: &str);

    fn pop_tag(&mut self, tag: HtmlTag) -> HtmlParseResult<()> {
        let popped = self
            ._pop_tag()
            .ok_or(HtmlParseError::new("Closing a tag, but none open"))?;
        if tag == popped {
            return Ok(());
        }
        let parent = self._last_tag();
        if parent == tag && popped.auto_close_when_parent(parent) {
            return Ok(());
        }
        Err(HtmlParseError::new(format!(
            "Expected tag '{tag}', but found stack '{} > {popped}'",
            self.stack_info()
        )))
    }
}

const LOG: bool = false;

pub(super) fn log<F: FnOnce() -> String>(index: NodeIndex, f: F) {
    LOG.then(|| println!("DOM[{index:>4}    ] {}", f()));
}

pub(super) fn log_opt_i<F: FnOnce() -> String>(index: Option<NodeIndex>, f: F) {
    LOG.then(|| {
        let index = index
            .map(|i| i.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("DOM[{index:>4}    ] {}", f());
    });
}

pub(super) fn log_list<I: Display, F: FnOnce() -> String>(index: NodeIndex, list: Option<I>, f: F) {
    LOG.then(|| {
        let list = list
            .map(|i| i.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("DOM[{index:>4},{list:>3}] {}", f());
    });
}
