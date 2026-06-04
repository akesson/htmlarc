use std::fmt::Display;

use indexmap::IndexSet;
use thiserror::Error;

use crate::css::{IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};

#[derive(Debug, PartialEq, Hash, Eq, Clone)]
pub enum CssChar {
    Alphabetic,
    Digit,
    Special(char),
}

impl Display for CssChar {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CssChar::Alphabetic => write!(f, "Alphabetic"),
            CssChar::Digit => write!(f, "Numeric"),
            CssChar::Special(c) => write!(f, "{}", c),
        }
    }
}

impl CssChar {
    pub fn from_char(c: char) -> Self {
        if c.is_ascii_alphabetic() {
            Self::Alphabetic
        } else if c.is_ascii_digit() {
            Self::Digit
        } else {
            Self::Special(c)
        }
    }
}

#[derive(Debug)]
pub struct CharList(Vec<CssChar>);

impl Display for CharList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}]",
            self.0
                .iter()
                .map(|c| c.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )?;

        Ok(())
    }
}

trait ToCharList {
    fn to_char_list(&self) -> CharList;
}

impl From<Vec<char>> for CharList {
    fn from(chars: Vec<char>) -> Self {
        CharList(chars.into_iter().map(CssChar::from_char).collect())
    }
}

impl From<Vec<CssChar>> for CharList {
    fn from(chars: Vec<CssChar>) -> Self {
        CharList(chars)
    }
}

impl ToCharList for IndexSet<CssChar> {
    fn to_char_list(&self) -> CharList {
        self.iter().cloned().collect::<Vec<_>>().into()
    }
}

#[derive(Debug, Error)]
pub enum TextPatternError {
    #[error("Can't start with a '{1}' at {0}")]
    StartsWith(usize, char),
    #[error("Can't be exclusively any of : {1} at {0}")]
    Exclusively(usize, CharList),
}

impl From<TextPatternError> for ParseError {
    fn from(val: TextPatternError) -> Self {
        val.into_parse_error()
    }
}

impl TextPatternError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for TextPatternError {
    fn index(&self) -> usize {
        match self {
            TextPatternError::StartsWith(index, _) => *index,
            TextPatternError::Exclusively(index, _) => *index,
        }
    }
}

#[derive(Default)]
pub struct TextPattern {
    allowed: IndexSet<CssChar>,
    start_with: IndexSet<CssChar>,
    not_exclusively: IndexSet<CssChar>,
    stop_at: IndexSet<char>,
}

impl TextPattern {
    pub fn allow_alphabetic(mut self) -> Self {
        self.allowed.insert(CssChar::Alphabetic);
        self
    }
    pub fn allow_numeric(mut self) -> Self {
        self.allowed.insert(CssChar::Digit);
        self
    }
    pub fn start_with(mut self, css_char: CssChar) -> Self {
        self.start_with.insert(css_char);
        self
    }
    pub fn not_exclusively(mut self, css_char: CssChar) -> Self {
        self.not_exclusively.insert(css_char);
        self
    }
    pub fn allow_special(mut self, char: char) -> Self {
        self.allowed.insert(CssChar::Special(char));
        self
    }
    pub fn stop_at(mut self, char: char) -> Self {
        self.stop_at.insert(char);
        self
    }

    pub fn validate<'s>(&self, chars: &mut CssChars<'s>) -> ParseResult<Option<&'s str>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No text pattern found at {}", chars.last_index());
            return Ok(None);
        };

        if self.stopped_at(start_char) {
            debug!("Stopped before finding text pattern at {}", start_index);
            return Ok(None);
        }

        if !self.is_allowed_start(start_char) {
            return Err(TextPatternError::StartsWith(start_index, start_char).into());
        }

        let mut end_index = start_index;
        let mut invalid = true;

        if self.is_allowed_char(start_char) && self.is_not_exclusively(start_char) {
            invalid = false;
        }

        for (i, c) in chars.by_ref() {
            if self.stopped_at(c) {
                debug!("Stopped at '{c}' {}", i);
                break;
            }

            if self.is_allowed_char(c) {
                if invalid && self.is_not_exclusively(c) {
                    invalid = false;
                }
                end_index = i;
            } else {
                break;
            }
        }

        if invalid {
            return Err(TextPatternError::Exclusively(
                start_index,
                self.not_exclusively.to_char_list(),
            )
            .into());
        }

        let range = start_index..=end_index;
        let value = chars.str(range);

        debug!("Parsed text pattern at {}", chars.last_index());
        Ok(Some(value))
    }

    fn is_allowed_start(&self, c: char) -> bool {
        self.start_with.contains(&CssChar::from_char(c))
    }

    fn is_allowed_char(&self, c: char) -> bool {
        self.allowed.contains(&CssChar::from_char(c))
    }

    fn stopped_at(&self, c: char) -> bool {
        self.stop_at.contains(&c)
    }

    fn is_not_exclusively(&self, c: char) -> bool {
        !self.not_exclusively.contains(&CssChar::from_char(c))
    }
}

#[test]
fn test_parse_text_pattern() {
    fn test_ok(string: &str, expected: Option<&str>) {
        debug!("\nTesting ok: '{}'", string);
        let mut chars = CssChars::new(string);
        let pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Digit)
            .allow_special('-')
            .not_exclusively(CssChar::Digit)
            .stop_at('r');
        let result = pattern.validate(&mut chars).unwrap();
        assert_eq!(result, expected);
    }

    test_ok("", None);
    test_ok("1abc", Some("1abc"));
    test_ok("1data-attr", Some("1data-att"));
    test_ok("2list_id", Some("2list"));

    fn test_err(string: &str, expected: ParseError) {
        debug!("\nTesting err: '{}'", string);
        let mut chars = CssChars::new(string);
        let pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Digit)
            .not_exclusively(CssChar::Digit);
        let result = pattern.validate(&mut chars).unwrap_err();
        assert_eq!(result.to_string(), expected.to_string());
    }

    test_err("abc", TextPatternError::StartsWith(0, 'a').into());
    test_err(
        "123",
        TextPatternError::Exclusively(0, CharList([CssChar::Digit].to_vec())).into(),
    );
}
