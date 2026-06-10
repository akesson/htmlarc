use std::borrow::Cow;
use std::fmt::Display;

use thiserror::Error;

use crate::css::{IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};
use crate::entities;

use super::CssPattern;

#[derive(Debug, Error)]
pub enum QuotedStringError {
    #[error("Empty quoted string at {0}")]
    Empty(usize),
    #[error("Unterminated quoted string at {0}")]
    Unterminated(usize),
}

impl From<QuotedStringError> for ParseError {
    fn from(val: QuotedStringError) -> Self {
        val.into_parse_error()
    }
}

impl QuotedStringError {
    fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for QuotedStringError {
    fn index(&self) -> usize {
        match *self {
            QuotedStringError::Empty(index) => index,
            QuotedStringError::Unterminated(index) => index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuotedString<'s>(pub Cow<'s, str>);

impl<'s> From<&'s str> for QuotedString<'s> {
    fn from(s: &'s str) -> Self {
        QuotedString(Cow::Borrowed(s))
    }
}

impl Display for QuotedString<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "'{}'", self.0)
    }
}

impl<'s> CssPattern<'s> for QuotedString<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        QuotedString::from_chars(chars)
    }
}

impl<'s> QuotedString<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((start_quote_index, start_quote)) = chars.current() else {
            debug!("No quoted string found at {}", chars.last_index());
            return Ok(None);
        };

        if start_quote != '"' && start_quote != '\'' {
            debug!("No quote opening found at {}", start_quote_index);
            return Ok(None);
        }

        while let Some((i, c)) = chars.next() {
            if c == start_quote {
                chars.next();
                if start_quote_index == i - 1 {
                    debug!("Empty quoted string at {}", i);
                    return Ok(Some(QuotedString(Cow::Borrowed(""))));
                }

                debug!("Parsed quoted string at {}", i);
                // Hoist: the selector literal is entity-decoded ONCE here, at parse time,
                // so every match path (attr / data-attr / class / text) compares against
                // the decoded form with zero per-match cost. `decode` borrows when there
                // is no entity, so the common case stays zero-copy.
                return Ok(Some(QuotedString(entities::decode(
                    chars.str(start_quote_index + 1..i),
                ))));
            }
        }

        Err(QuotedStringError::Unterminated(chars.last_index()).into())
    }
}

#[test]
fn test_parse_quoted_string() {
    use crate::css::helpers::test_ok;

    test_ok("", None::<QuotedString>);
    test_ok("''", Some(QuotedString("".into())));
    test_ok("\"abc\"", Some(QuotedString("abc".into())));
    test_ok("'abc'", Some(QuotedString("abc".into())));
    test_ok("]", None::<QuotedString>);
    test_ok("\"\"", Some(QuotedString("".into())));
    test_ok("'hi'", Some(QuotedString("hi".into())));

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<QuotedString>(string, expected);
    }

    test_err("\"", QuotedStringError::Unterminated(0).into());
    test_err("\"abc", QuotedStringError::Unterminated(3).into());
}
