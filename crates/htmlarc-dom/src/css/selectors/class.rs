use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{CssChar, CssPattern, TextPattern},
    },
    stores::Class,
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

#[derive(Debug, Clone, Copy)]
pub struct ClassSelector<'s>(pub &'s str);

impl Display for ClassSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, ".{}", self.0)
    }
}

impl PartialEq<Class<'_>> for ClassSelector<'_> {
    fn eq(&self, other: &Class<'_>) -> bool {
        self.0 == other.0
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
            Ok(Some(ClassSelector(id)))
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
    test_ok(".-hyphen", Some(ClassSelector("-hyphen")));
    test_ok("._underscore", Some(ClassSelector("_underscore")));
    test_ok(".withdigit1", Some(ClassSelector("withdigit1")));
    test_ok(
        ".hyphen-_underscore",
        Some(ClassSelector("hyphen-_underscore")),
    );
    test_ok(".stop[", Some(ClassSelector("stop")));

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
