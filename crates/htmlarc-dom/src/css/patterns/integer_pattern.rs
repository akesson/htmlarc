use thiserror::Error;

use crate::css::{IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};

#[derive(Debug, Error)]
pub enum IntegerPatternError {
    #[error("Failed to parse integer at {0}: {1}")]
    ParseFail(usize, String),
}

impl From<IntegerPatternError> for ParseError {
    fn from(val: IntegerPatternError) -> Self {
        val.into_parse_error()
    }
}

impl IntegerPatternError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for IntegerPatternError {
    fn index(&self) -> usize {
        match *self {
            IntegerPatternError::ParseFail(index, _) => index,
        }
    }
}

pub struct IntegerPattern;

impl IntegerPattern {
    pub fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<usize>> {
        let Some((index, char)) = chars.current() else {
            debug!("No integer found at {}", chars.last_index());
            return Ok(None);
        };

        if !char.is_ascii_digit() {
            debug!("Not an integer at {}", index);
            return Ok(None);
        }

        let mut int = String::from(char);

        for (_, char) in chars.by_ref() {
            if char.is_ascii_digit() {
                int.push(char);
            } else {
                break;
            }
        }

        debug!("Parsing integer '{}' at {}", int, index);
        let int = match int.parse::<usize>() {
            Ok(int) => int,
            Err(e) => {
                return Err(IntegerPatternError::ParseFail(index, e.to_string()).into());
            }
        };

        Ok(Some(int))
    }
}

#[test]
fn test_parse_integer_pattern() {
    fn test_ok(string: &str, expected: Option<usize>) {
        debug!("\nTesting ok: '{}'", string);
        let mut chars = CssChars::new(string);
        let result = IntegerPattern::from_chars(&mut chars).unwrap();

        assert_eq!(result, expected);
    }

    test_ok("", None);
    test_ok("n", None);
    test_ok("123", Some(123));
    test_ok("456abc", Some(456));

    fn test_err(string: &str, expected: ParseError) {
        debug!("\nTesting err: '{}'", string);
        let mut chars = CssChars::new(string);
        let result = IntegerPattern::from_chars(&mut chars).unwrap_err();

        assert_eq!(result.to_string(), expected.to_string());
    }

    test_err(
        "18446744073709551616",
        IntegerPatternError::ParseFail(0, "number too large to fit in target type".to_string())
            .into(),
    );
}
