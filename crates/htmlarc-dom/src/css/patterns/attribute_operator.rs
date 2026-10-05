use std::fmt::Display;

use thiserror::Error;

use crate::css::{IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug};

use super::CssPattern;

#[derive(Debug, Error)]
pub enum AttributeOperatorError {
    #[error("Invalid attribute operator at {0}")]
    InvalidAttributeOperator(usize),
    #[error("Missing equal sign at {0}")]
    MissingEqualSign(usize),
}

impl From<AttributeOperatorError> for ParseError {
    fn from(val: AttributeOperatorError) -> Self {
        val.into_parse_error()
    }
}

impl AttributeOperatorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for AttributeOperatorError {
    fn index(&self) -> usize {
        match *self {
            AttributeOperatorError::InvalidAttributeOperator(index) => index,
            AttributeOperatorError::MissingEqualSign(index) => index,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum AttributeOperator {
    /// =
    Exact,
    /// ^=
    Starts,
    /// *=
    Includes,
    /// $=
    Ends,
    /// ~=
    List,
    /// |=
    DashMatch,
}

impl Display for AttributeOperator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributeOperator::Exact => write!(f, "="),
            AttributeOperator::Starts => write!(f, "^="),
            AttributeOperator::Includes => write!(f, "*="),
            AttributeOperator::Ends => write!(f, "$="),
            AttributeOperator::List => write!(f, "~="),
            AttributeOperator::DashMatch => write!(f, "|="),
        }
    }
}

impl<'s> CssPattern<'s> for AttributeOperator {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl AttributeOperator {
    fn from_chars(chars: &mut CssChars) -> ParseResult<Option<Self>> {
        let Some((start_index, start_char)) = chars.current() else {
            debug!("No attribute operator found at {}", chars.last_index());
            return Ok(None);
        };

        if start_char == ']' {
            debug!("Attribute operator is empty at {}", start_index);
            return Ok(None);
        }

        debug!("Checking for exact attribute operator at {}", start_index);
        if start_char == '=' {
            chars.next();
            debug!("Parse attribute operator: {}", AttributeOperator::Exact);
            return Ok(Some(AttributeOperator::Exact));
        }

        debug!("Checking for other attribute operators at {}", start_index);
        let operator = match start_char {
            '^' => AttributeOperator::Starts,
            '*' => AttributeOperator::Includes,
            '$' => AttributeOperator::Ends,
            '~' => AttributeOperator::List,
            '|' => AttributeOperator::DashMatch,
            _ => return Err(AttributeOperatorError::InvalidAttributeOperator(start_index).into()),
        };
        debug!("Expecting attribute operator: {}", operator);

        let Some((next_index, next_char)) = chars.next() else {
            return Err(AttributeOperatorError::MissingEqualSign(start_index + 1).into());
        };

        if next_char != '=' {
            return Err(AttributeOperatorError::MissingEqualSign(next_index).into());
        }
        debug!("Parsed attribute operator: {}", operator);

        chars.next();

        Ok(Some(operator))
    }

    /// Whether `value` matches `pattern` under this operator. `ascii_ci` compares ASCII
    /// case-insensitively, as CSS does for the `i` flag and the case-insensitive attributes;
    /// non-ASCII characters still compare exactly. Neither mode allocates.
    pub fn matches(&self, pattern: &str, value: &str, ascii_ci: bool) -> bool {
        use AttributeOperator::*;
        let eq = |a: &str, b: &str| {
            if ascii_ci {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        let (p, v) = (pattern.as_bytes(), value.as_bytes());
        let starts = || v.len() >= p.len() && eq_bytes(&v[..p.len()], p, ascii_ci);
        match self {
            Exact => eq(pattern, value),
            Starts => starts(),
            Includes => {
                if ascii_ci {
                    p.is_empty() || v.windows(p.len()).any(|w| w.eq_ignore_ascii_case(p))
                } else {
                    value.contains(pattern)
                }
            }
            Ends => v.len() >= p.len() && eq_bytes(&v[v.len() - p.len()..], p, ascii_ci),
            List => value.split_whitespace().any(|v| eq(v, pattern)),
            DashMatch => eq(pattern, value) || (starts() && v.get(p.len()) == Some(&b'-')),
        }
    }
}

fn eq_bytes(a: &[u8], b: &[u8], ascii_ci: bool) -> bool {
    if ascii_ci {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

#[test]
fn test_parse_attribute_operator() {
    use crate::css::helpers::test_ok;

    test_ok("=", Some(AttributeOperator::Exact));
    test_ok("^=", Some(AttributeOperator::Starts));
    test_ok("*=", Some(AttributeOperator::Includes));
    test_ok("$=", Some(AttributeOperator::Ends));
    test_ok("~=", Some(AttributeOperator::List));
    test_ok("|=", Some(AttributeOperator::DashMatch));
    test_ok("", None::<AttributeOperator>);

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<AttributeOperator>(string, expected);
    }

    test_err("^", AttributeOperatorError::MissingEqualSign(1).into());
    test_err("*a", AttributeOperatorError::MissingEqualSign(1).into());
}
