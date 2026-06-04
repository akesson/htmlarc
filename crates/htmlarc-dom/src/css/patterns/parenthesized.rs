use std::{
    fmt::{Debug, Display},
    marker::PhantomData,
};

use thiserror::Error;

use crate::css::{Context, IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};

use super::CssPattern;

#[derive(Debug, Error)]
pub enum ParenthesizedError {
    #[error("Invalid content at {0}")]
    InvalidContent(usize),
    #[error("Invalid end delimiter at {0}: expected '{1}', found '{2}'")]
    InvalidEndDelimiter(usize, char, char),
    #[error("Missing content at {0}")]
    MissingContent(usize),
    #[error("Missing end delimiter '{1}' at {0}")]
    MissingEndDelimiter(usize, char),
}

impl From<ParenthesizedError> for ParseError {
    fn from(val: ParenthesizedError) -> Self {
        val.into_parse_error()
    }
}

impl ParenthesizedError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for ParenthesizedError {
    fn index(&self) -> usize {
        match *self {
            ParenthesizedError::InvalidContent(index) => index,
            ParenthesizedError::InvalidEndDelimiter(index, _, _) => index,
            ParenthesizedError::MissingContent(index) => index,
            ParenthesizedError::MissingEndDelimiter(index, _) => index,
        }
    }
}

#[derive(Debug)]
pub struct Parentheses;

impl Display for Parentheses {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Parentheses")
    }
}

impl Delimiter for Parentheses {
    fn open() -> char {
        '('
    }

    fn close() -> char {
        ')'
    }
}

#[derive(Debug)]
pub struct Brackets;

impl Display for Brackets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Brackets")
    }
}

impl Delimiter for Brackets {
    fn open() -> char {
        '['
    }

    fn close() -> char {
        ']'
    }
}

pub trait Delimiter: Debug {
    fn open() -> char;
    fn close() -> char;
}

#[derive(Debug)]
pub struct Parenthesized<D, T> {
    _delimiter: PhantomData<D>,
    content: Option<T>,
}

impl<'s, D: Delimiter + Display, T: CssPattern<'s>> Display for Parenthesized<D, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(content) = &self.content {
            write!(f, "{}{}{}", D::open(), content, D::close())
        } else {
            write!(f, "{}{}", D::open(), D::close())
        }
    }
}

impl<'s, D: Delimiter + Display, T: CssPattern<'s>> CssPattern<'s> for Parenthesized<D, T> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s, D: Delimiter, T: CssPattern<'s>> Parenthesized<D, T> {
    pub fn inner(self) -> Option<T> {
        self.content
    }
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No parenthesized found at {}", chars.last_index());
            return Ok(None);
        };

        if start_char == D::open() {
            if let Some((i, _)) = chars.next() {
                debug!("Parsing parenthesized content at {}", start_index);
                let content =
                    T::from_chars(chars).context(ParenthesizedError::InvalidContent(i))?;

                debug!(
                    "Checking for parenthesized end delimiter at {}",
                    chars.last_index()
                );
                if let Some((end_index, end_char)) = chars.current() {
                    if end_char == D::close() {
                        chars.next();
                        Ok(Some(Self {
                            _delimiter: PhantomData,
                            content,
                        }))
                    } else {
                        Err(ParenthesizedError::InvalidEndDelimiter(
                            end_index,
                            D::close(),
                            end_char,
                        )
                        .into())
                    }
                } else {
                    Err(
                        ParenthesizedError::MissingEndDelimiter(chars.last_index() + 1, D::close())
                            .into(),
                    )
                }
            } else {
                Err(ParenthesizedError::MissingContent(start_index).into())
            }
        } else {
            debug!("No parenthesized openning found at {}", start_index);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_parenthesized() {
    use crate::css::patterns::{CssChar, TextPattern, TextPatternError};

    #[derive(Debug, Eq, PartialEq)]
    pub struct AlphaNumDashUnderscore<'s>(&'s str);

    impl Display for AlphaNumDashUnderscore<'_> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl<'s> CssPattern<'s> for AlphaNumDashUnderscore<'s> {
        fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
            Self::from_chars(chars)
        }
    }

    impl<'s> AlphaNumDashUnderscore<'s> {
        pub fn new(value: &'s str) -> Self {
            Self(value)
        }
        fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
            let pattern = TextPattern::default()
                .allow_alphabetic()
                .allow_numeric()
                .allow_special('-')
                .allow_special('_')
                .start_with(CssChar::Alphabetic)
                .start_with(CssChar::Special('-'))
                .start_with(CssChar::Special('_'))
                .stop_at('(')
                .stop_at(')')
                .stop_at('[')
                .stop_at(']')
                .stop_at(' ')
                .stop_at(':')
                .stop_at('.')
                .stop_at('#')
                .not_exclusively(CssChar::Special('-'))
                .not_exclusively(CssChar::Special('_'));

            if let Some(text) = pattern.validate(chars)? {
                Ok(Some(Self(text)))
            } else {
                Ok(None)
            }
        }
    }

    fn test_ok<'s, D: Delimiter + Display, T: CssPattern<'s>>(
        string: &'s str,
        expected: Option<Parenthesized<D, T>>,
    ) {
        crate::css::helpers::test_ok(string, expected);
    }
    test_ok::<Parentheses, AlphaNumDashUnderscore>(
        "()",
        Some(Parenthesized {
            _delimiter: PhantomData,
            content: None,
        }),
    );

    test_ok::<Parentheses, AlphaNumDashUnderscore>(
        "(a)",
        Some(Parenthesized {
            _delimiter: PhantomData,
            content: Some(AlphaNumDashUnderscore::new("a")),
        }),
    );

    fn test_err<'s, D: Delimiter + Display, T: CssPattern<'s>>(
        string: &'s str,
        expected: ParseError,
    ) {
        crate::css::helpers::test_err::<Parenthesized<D, T>>(string, expected);
    }

    test_err::<Parentheses, AlphaNumDashUnderscore>(
        "(",
        ParenthesizedError::MissingContent(0).into(),
    );
    test_err::<Parentheses, AlphaNumDashUnderscore>(
        "(html",
        ParenthesizedError::MissingEndDelimiter(5, ')').into(),
    );
    test_err::<Parentheses, AlphaNumDashUnderscore>(
        "(html]",
        ParenthesizedError::InvalidEndDelimiter(5, ')', ']').into(),
    );
    test_err::<Parentheses, AlphaNumDashUnderscore>(
        "(5html)",
        ParseError::Context(
            ParenthesizedError::InvalidContent(1).into(),
            std::convert::Into::<ParseError>::into(TextPatternError::StartsWith(1, '5')).into(),
        ),
    );
}
