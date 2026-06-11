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
    html::HtmlAttr,
};

/// Per-document resolution of an id or attribute-name selector to an integer ref — an
/// attribute *entry id* for `#id`, a *`NameSym`* for `[attr]` — set by the resolve pass that
/// [`MatchIter`](crate::iters::MatchIter) runs once when it binds a selector list to a
/// document (ADR 0002 §3). It turns per-node matching into integer compares.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ResolvedRef {
    /// No resolve pass has run — match by string comparison (the direct-matching paths
    /// `Element::matches`/`Selector::matches` that bypass `MatchIter`).
    #[default]
    Unresolved,
    /// The ref is present in the document — match by integer compare.
    Found(u16),
    /// Absent from the document, so this selector can never match here (correct through
    /// `:not`, which negates the inner result).
    Absent,
}

#[derive(Debug, Error)]
pub enum IdSelectorError {
    #[error("Failed to parse id selector at {0}")]
    ParseFail(usize),
    #[error("Missing id name at {0}")]
    MissingId(usize),
}

impl From<IdSelectorError> for ParseError {
    fn from(val: IdSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl IdSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for IdSelectorError {
    fn index(&self) -> usize {
        match *self {
            IdSelectorError::ParseFail(index) => index,
            IdSelectorError::MissingId(index) => index,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdSelector<'s> {
    pub name: &'s str,
    pub(crate) resolved: ResolvedRef,
}

impl<'s> IdSelector<'s> {
    pub fn new(name: &'s str) -> Self {
        Self {
            name,
            resolved: ResolvedRef::Unresolved,
        }
    }

    /// Bind this selector to a document by resolving its id against the attribute store.
    /// `#id` matches the `id` attribute by exact, case-sensitive value, so resolve the value
    /// string to a `ValueRef` and then to the `(id, value)` entry; a value that the document
    /// never stored (or never paired with `id`) makes the selector `Absent` (ADR 0002 §3).
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        self.resolved = match view.attrs.value_ref(self.name) {
            Some(vref) => match view.attrs.find_entry((HtmlAttr::id as u16, vref)) {
                Some(entry) => ResolvedRef::Found(entry),
                None => ResolvedRef::Absent,
            },
            None => ResolvedRef::Absent,
        };
    }
}

impl Display for IdSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.name)
    }
}

impl<'s> CssPattern<'s> for IdSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> IdSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No id selector found at {}", chars.last_index());
            return Ok(None);
        };

        if start_char != '#' {
            debug!("Not an id selector at {}", start_index);
            return Ok(None);
        }

        chars.next();

        let id_pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Alphabetic)
            .start_with(CssChar::Special('_'))
            .start_with(CssChar::Special('-'))
            .allow_special('-')
            .allow_special('_')
            .not_exclusively(CssChar::Special('-'))
            .not_exclusively(CssChar::Special('_'))
            .stop_at('[')
            .stop_at('#')
            .stop_at(':')
            .stop_at('.')
            .stop_at(' ')
            .stop_at('\n');

        debug!("Parsing id selector name at {}", start_index);
        if let Some(id) = id_pattern
            .validate(chars)
            .context(IdSelectorError::ParseFail(start_index))?
        {
            debug!("Parsed id selector at: {}", chars.last_index());
            Ok(Some(IdSelector::new(id)))
        } else {
            Err(IdSelectorError::MissingId(start_index).into())
        }
    }
}

#[test]
fn test_parse_id_selector() {
    use crate::css::{ParseError, helpers::test_ok, patterns::TextPatternError};

    test_ok("", None::<IdSelector>);
    test_ok(".", None::<IdSelector>);
    test_ok("#-hyphen", Some(IdSelector::new("-hyphen")));
    test_ok("#_underscore", Some(IdSelector::new("_underscore")));
    test_ok("#withdigit1", Some(IdSelector::new("withdigit1")));
    test_ok(
        "#hyphen-_underscore",
        Some(IdSelector::new("hyphen-_underscore")),
    );
    test_ok("#stop[", Some(IdSelector::new("stop")));

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<IdSelector>(string, expected);
    }

    test_err("#", IdSelectorError::MissingId(0).into());
    test_err(
        "#3",
        ParseError::new(TextPatternError::StartsWith(1, '3'))
            .context(IdSelectorError::ParseFail(0)),
    );
    test_err(
        "#4id",
        ParseError::new(TextPatternError::StartsWith(1, '4'))
            .context(IdSelectorError::ParseFail(0)),
    );
    test_err(
        "#--",
        ParseError::new(TextPatternError::Exclusively(1, ['-', '_'].to_vec().into()))
            .context(IdSelectorError::ParseFail(0)),
    );
}
