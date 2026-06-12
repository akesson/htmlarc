use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{CssChar, CssPattern, TextPattern},
    },
    dom::DomView,
    html::HtmlTag,
    stores::Sym,
};

#[derive(Debug, Error)]
pub enum TagSelectorError {
    #[error("Failed to parse tag selector at {0}")]
    ParseFail(usize),
}

impl From<TagSelectorError> for ParseError {
    fn from(value: TagSelectorError) -> Self {
        value.into_parse_error()
    }
}

impl TagSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for TagSelectorError {
    fn index(&self) -> usize {
        match *self {
            TagSelectorError::ParseFail(index) => index,
        }
    }
}

/// A parsed type/tag selector: a recognised [`HtmlTag`], or an *extended* (custom/unknown)
/// element name kept verbatim with its resolve-once binding. Mirrors
/// [`crate::stores::AttrName`] for attribute names (ADR 0002 §4). An unknown name is no
/// longer a parse error — it becomes an extended selector that matches nothing unless the
/// document actually holds that custom element.
#[derive(Debug, Clone, Copy)]
pub enum TagSelector<'s> {
    Std(HtmlTag),
    Ext(ExtTagSelector<'s>),
}

/// An extended (custom-element / unknown) tag selector and its resolve-once binding.
#[derive(Debug, Clone, Copy)]
pub struct ExtTagSelector<'s> {
    pub(crate) name: &'s str,
    pub(crate) resolved: ResolvedTag,
}

/// Per-document resolution of an extended tag selector (ADR 0002 §4), set by the resolve pass
/// [`MatchIter`](crate::iters::MatchIter) runs once when it binds a selector list to a
/// document. Turns per-node tag matching into a byte compare.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ResolvedTag {
    /// No resolve pass has run — match by string comparison of the node's tag name (the
    /// direct `Element::matches`/`matches_css` paths that bypass `MatchIter`).
    #[default]
    Unresolved,
    /// The name is a vocab tag in this document — match by node tag-byte equality.
    Byte(u8),
    /// The name is an overflow tag in this document — match `byte == EXT_OVERFLOW`, then a
    /// side-map lookup confirming the symbol.
    OverflowSym(Sym),
    /// Absent from this document, so the selector never matches here (correct through `:not`,
    /// which negates the inner result).
    Absent,
}

impl<'s> ExtTagSelector<'s> {
    pub(crate) fn new(name: &'s str) -> Self {
        Self {
            name,
            resolved: ResolvedTag::Unresolved,
        }
    }

    /// Bind this selector to a document once (ADR 0002 §4): resolve the name to its vocab byte
    /// (the common case → a per-node byte compare), to an overflow symbol, or to `Absent`.
    /// A name present in the symbol table only as a class token / attribute name (not a tag)
    /// resolves to `Absent` when nothing overflowed, else to `OverflowSym` — still correct, as
    /// its symbol is never in the overflow map, so no node matches.
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        // Stored tag names are lowercase (html5gum lowercases everything); a CSS type
        // selector is ASCII-case-insensitive, so a camelCase SVG spelling like `clipPath`
        // must resolve to the lowercased symbol (ADR 0002 §5).
        let found = if self.name.bytes().any(|b| b.is_ascii_uppercase()) {
            view.symbols.find(&self.name.to_ascii_lowercase())
        } else {
            view.symbols.find(self.name)
        };
        self.resolved = match found {
            None => ResolvedTag::Absent,
            Some(sym) => match view.ext_tags.vocab_byte(sym) {
                Some(byte) => ResolvedTag::Byte(byte),
                None if view.ext_tags.overflow_is_empty() => ResolvedTag::Absent,
                None => ResolvedTag::OverflowSym(sym),
            },
        };
    }
}

impl Display for TagSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagSelector::Std(tag) => write!(f, "{tag}"),
            TagSelector::Ext(ext) => write!(f, "{ext}"),
        }
    }
}

impl Display for ExtTagSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl<'s> CssPattern<'s> for TagSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> TagSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((tag_index, _)) = chars.current() else {
            debug!("No tag selector found at {}", chars.last_index());
            return Ok(None);
        };

        let pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Alphabetic)
            .allow_special('-')
            .allow_special('_')
            .stop_at('[')
            .stop_at('#')
            .stop_at(':')
            .stop_at('.')
            .stop_at(' ')
            .stop_at('\n');

        debug!("Parsing tag name at {}", tag_index);
        if let Some(tag) = pattern
            .validate(chars)
            .context(TagSelectorError::ParseFail(tag_index))?
        {
            // A recognised element name binds to its `HtmlTag`; anything else — a custom
            // element or a reserved system spelling — is an extended selector (ADR 0002 §4).
            Ok(Some(match HtmlTag::from_tag_name(tag) {
                Some(std) => TagSelector::Std(std),
                None => TagSelector::Ext(ExtTagSelector::new(tag)),
            }))
        } else {
            debug!("No tag selector found at {}", tag_index);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_tag_selector() {
    use crate::css::{ParseError, helpers::test_ok, patterns::TextPatternError};

    test_ok("", None::<TagSelector>);
    test_ok("div", Some(TagSelector::Std(HtmlTag::div)));
    test_ok("h1", Some(TagSelector::Std(HtmlTag::h1)));
    // `figure-inline` and `hnan` were demoted from the enum (ADR 0002 §4) and now parse as
    // extended selectors, like any custom element.
    test_ok(
        "figure-inline",
        Some(TagSelector::Ext(ExtTagSelector::new("figure-inline"))),
    );
    // An unknown name is no longer a parse error — it is an extended selector (matches
    // nothing unless the document holds that custom element).
    test_ok("dib", Some(TagSelector::Ext(ExtTagSelector::new("dib"))));
    test_ok(
        "my-widget",
        Some(TagSelector::Ext(ExtTagSelector::new("my-widget"))),
    );
    // The `extended` marker spelling routes to an extended selector, never the enum variant.
    test_ok(
        "extended",
        Some(TagSelector::Ext(ExtTagSelector::new("extended"))),
    );
    test_ok("span.inline", Some(TagSelector::Std(HtmlTag::span)));

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<TagSelector>(string, expected);
    }

    test_err(
        "1div",
        ParseError::new(TextPatternError::StartsWith(0, '1'))
            .context(TagSelectorError::ParseFail(0)),
    );
}
