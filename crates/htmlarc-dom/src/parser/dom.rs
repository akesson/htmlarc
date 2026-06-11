use std::fmt::Display;

use crate::dom::NodeIndex;
use crate::html::HtmlTag;
use crate::stores::AttrName;

use crate::{HtmlParseError, HtmlParseResult};

/// A parsed tag name: a recognised [`HtmlTag`], or an *extended* (custom/unknown) name kept
/// verbatim. Mirrors [`AttrName`] for attribute names (ADR 0002 §3–§4). The `extended` marker
/// and the reserved system spellings can never be produced as `Std` — see
/// [`HtmlTag::from_tag_name`].
#[derive(Clone, Copy)]
pub(crate) enum TagName<'a> {
    Std(HtmlTag),
    Ext(&'a str),
}

impl<'a> TagName<'a> {
    /// Parse a tag-name string into a recognised element or an extended name.
    pub(crate) fn parse(name: &'a str) -> TagName<'a> {
        match HtmlTag::from_tag_name(name) {
            Some(tag) => TagName::Std(tag),
            None => TagName::Ext(name),
        }
    }
}

impl Display for TagName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagName::Std(tag) => write!(f, "{tag}"),
            TagName::Ext(name) => write!(f, "{name}"),
        }
    }
}

pub(crate) trait DomStack {
    /// The builder's stack token. It carries full tag *identity* — a symbol for the real
    /// builder, a string for the test DOM — so two distinct extended tags, which share the
    /// `HtmlTag::extended` kind, never close one another (`</a-a>` cannot close `<b-b>`).
    type Tag: Clone + PartialEq;

    /// Construct a stack token from a parsed name, interning an extended name into the
    /// document symbol table (a standard name carries its `HtmlTag` directly).
    fn make_tag(&mut self, name: TagName<'_>) -> Self::Tag;

    /// The normalized [`HtmlTag`] kind of a stack token (`extended` for a custom element), for
    /// the auto-close classifiers.
    fn kind_of(tag: &Self::Tag) -> HtmlTag;

    /// Render a stack token's name for a parse-error message.
    fn tag_display(&self, tag: &Self::Tag) -> String;

    /// Only for internal use
    fn _pop_tag(&mut self) -> Option<Self::Tag>;

    /// Only for internal use
    fn _last_tag(&self) -> Option<Self::Tag>;

    fn _push_tag(&mut self, tag: Self::Tag);

    fn stack_info(&self) -> String;

    /// Only for adding comments and text (always a system tag, never extended).
    fn add_text_tag(&mut self, tag: HtmlTag, value: &str);

    /// Add one attribute. Standard, `data-*`, and unknown names all flow through here as an
    /// [`AttrName`] (ADR 0002 §3) — the builder routes `class` to its run and interns
    /// extended names into the document symbol table.
    fn add_attribute(&mut self, name: AttrName<'_>, value: &str);

    fn push_tag(&mut self, name: TagName<'_>) {
        let tag = self.make_tag(name);
        let current = self
            ._last_tag()
            .map(|t| Self::kind_of(&t))
            .unwrap_or(HtmlTag::sys_root);
        if current.auto_close_when_next(Self::kind_of(&tag)) {
            self._pop_tag();
        }
        self._push_tag(tag)
    }

    fn pop_tag(&mut self, name: TagName<'_>) -> HtmlParseResult<()> {
        let tag = self.make_tag(name);
        let popped = self
            ._pop_tag()
            .ok_or(HtmlParseError::new("Closing a tag, but none open"))?;
        if tag == popped {
            return Ok(());
        }
        // The implied-end-tag rule: a `</parent>` may close a still-open child. Compare the
        // parent by full identity too, so an extended parent only satisfies its own name.
        if self._last_tag().as_ref() == Some(&tag)
            && Self::kind_of(&popped).auto_close_when_parent(Self::kind_of(&tag))
        {
            return Ok(());
        }
        Err(HtmlParseError::new(format!(
            "Expected tag '{name}', but found stack '{} > {}'",
            self.stack_info(),
            self.tag_display(&popped),
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
