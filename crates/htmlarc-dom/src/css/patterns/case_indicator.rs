use std::fmt::Display;

use thiserror::Error;

use crate::css::{IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};

use super::CssPattern;

#[derive(Debug, Error)]
pub enum CaseIndicatorError {
    #[error("Invalid case indicator at {0}")]
    InvalidCaseIndicator(usize),
    #[error("Missing case indicator at {0}")]
    MissingCaseIndicator(usize),
}

impl From<CaseIndicatorError> for ParseError {
    fn from(val: CaseIndicatorError) -> Self {
        val.into_parse_error()
    }
}

impl CaseIndicatorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for CaseIndicatorError {
    fn index(&self) -> usize {
        match *self {
            CaseIndicatorError::InvalidCaseIndicator(index) => index,
            CaseIndicatorError::MissingCaseIndicator(index) => index,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CaseIndicator {
    Sensitive,
    Insensitive,
}

impl Display for CaseIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CaseIndicator::Sensitive => write!(f, " s"),
            CaseIndicator::Insensitive => write!(f, " i"),
        }
    }
}

impl<'s> CssPattern<'s> for CaseIndicator {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        CaseIndicator::from_chars(chars)
    }
}

impl CaseIndicator {
    fn from_chars(chars: &mut CssChars) -> ParseResult<Option<Self>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No case indicator found at {}", chars.last_index());
            return Ok(None);
        };

        if start_char == ' ' {
            if let Some((next_index, next_char)) = chars.next() {
                if next_char == 'i' {
                    chars.next();
                    debug!("Parsed insensitive case indicator at {}", next_index);
                    Ok(Some(CaseIndicator::Insensitive))
                } else if next_char == 's' {
                    chars.next();
                    debug!("Parsed sensitive case indicator at {}", next_index);
                    Ok(Some(CaseIndicator::Sensitive))
                } else {
                    Err(CaseIndicatorError::InvalidCaseIndicator(next_index).into())
                }
            } else {
                Err(CaseIndicatorError::MissingCaseIndicator(start_index).into())
            }
        } else {
            debug!("No case indicator found at {}", start_index);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_case_indicator() {
    use crate::css::helpers::test_ok;

    test_ok(" i", Some(CaseIndicator::Insensitive));
    test_ok(" s", Some(CaseIndicator::Sensitive));
    test_ok("]", None::<CaseIndicator>);
    test_ok("", None::<CaseIndicator>);

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<CaseIndicator>(string, expected);
    }

    test_err(" x", CaseIndicatorError::InvalidCaseIndicator(1).into());
    test_err(" ", CaseIndicatorError::MissingCaseIndicator(0).into());
}
