use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{CssChar, CssPattern, TextPattern},
    },
    stores::{Class, Sym, SymbolTableView},
};

#[derive(Debug, Error)]
pub enum ClassSelectorError {
    #[error("Failed to parse class selector at {0}")]
    ParseFail(usize),
    #[error("Missing class name at {0}")]
    MissingClass(usize),
}

impl From<ClassSelectorError> for ParseError {
    fn from(val: ClassSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl ClassSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for ClassSelectorError {
    fn index(&self) -> usize {
        match *self {
            ClassSelectorError::ParseFail(index) => index,
            ClassSelectorError::MissingClass(index) => index,
        }
    }
}

/// Per-document resolution state of a class selector, set by the resolve pass that
/// [`MatchIter`](crate::iters::MatchIter) runs once when it binds a selector list to a
/// document. Resolving the class string to a stable [`Sym`] turns per-node matching into
/// integer compares (ADR 0002 §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ResolvedSym {
    /// No resolve pass has run — match by string comparison. This is the state for direct
    /// matching paths that bypass `MatchIter` (`Element::matches`, `Selector::matches`).
    #[default]
    Unresolved,
    /// The class is present in the document's symbol table — match by `Sym` compare.
    Found(Sym),
    /// The class is absent from the document, so this selector can never match here. This
    /// is semantically correct through `:not`, which negates the inner result.
    Absent,
}

#[derive(Debug, Clone, Copy)]
pub struct ClassSelector<'s> {
    pub name: &'s str,
    pub(crate) resolved: ResolvedSym,
}

impl<'s> ClassSelector<'s> {
    pub fn new(name: &'s str) -> Self {
        Self {
            name,
            resolved: ResolvedSym::Unresolved,
        }
    }

    /// Bind this selector to a document by resolving its class name against the symbol
    /// table (called by the [`MatchIter`](crate::iters::MatchIter) resolve pass).
    pub(crate) fn resolve(&mut self, symbols: SymbolTableView<'_>) {
        self.resolved = match symbols.find(self.name) {
            Some(sym) => ResolvedSym::Found(sym),
            None => ResolvedSym::Absent,
        };
    }
}

impl Display for ClassSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, ".{}", self.name)
    }
}

impl PartialEq<Class<'_>> for ClassSelector<'_> {
    fn eq(&self, other: &Class<'_>) -> bool {
        self.name == other.0
    }
}

impl<'s> CssPattern<'s> for ClassSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> ClassSelector<'s> {
    pub fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No class selector found at {}", chars.last_index());
            return Ok(None);
        };

        if start_char != '.' {
            debug!("Not a class selector at {}", start_index);
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

        debug!("Parsing class selector name at {}", start_index);
        if let Some(id) = id_pattern
            .validate(chars)
            .context(ClassSelectorError::ParseFail(start_index))?
        {
            debug!("Parsed class selector at {}", start_index);
            Ok(Some(ClassSelector::new(id)))
        } else {
            Err(ClassSelectorError::MissingClass(start_index).into())
        }
    }
}

#[test]
fn test_parse_class_selector() {
    use crate::css::{ParseError, helpers::test_ok, patterns::TextPatternError};

    test_ok("", None::<ClassSelector>);
    test_ok("#", None::<ClassSelector>);
    test_ok(".-hyphen", Some(ClassSelector::new("-hyphen")));
    test_ok("._underscore", Some(ClassSelector::new("_underscore")));
    test_ok(".withdigit1", Some(ClassSelector::new("withdigit1")));
    test_ok(
        ".hyphen-_underscore",
        Some(ClassSelector::new("hyphen-_underscore")),
    );
    test_ok(".stop[", Some(ClassSelector::new("stop")));

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<ClassSelector>(string, expected);
    }

    test_err(".", ClassSelectorError::MissingClass(0).into());
    test_err(
        ".3",
        ParseError::new(TextPatternError::StartsWith(1, '3'))
            .context(ClassSelectorError::ParseFail(0)),
    );
    test_err(
        ".4Class",
        ParseError::new(TextPatternError::StartsWith(1, '4'))
            .context(ClassSelectorError::ParseFail(0)),
    );
    test_err(
        ".--",
        ParseError::new(TextPatternError::Exclusively(1, ['-', '_'].to_vec().into()))
            .context(ClassSelectorError::ParseFail(0)),
    );
}
