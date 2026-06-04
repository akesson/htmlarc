use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, CssChar, IndexedError, ParseError, ParseResult, TextPattern,
        chars::CssChars,
        logging::{self, debug},
        patterns::integer_pattern::IntegerPattern,
    },
    dom::DomRead,
    html::HtmlElement,
};

use super::CssPattern;

#[derive(Debug, Error)]
pub enum OrdinalPatternError {
    #[error("Invalid ordinal pattern at {0}")]
    Invalid(usize),
    #[error("Invalid step at {0}")]
    InvalidStep(usize),
    #[error("Invalid offset at {0}")]
    InvalidOffset(usize),
    #[error("Unterminated ordinal pattern at {0}")]
    Unterminated(usize),
    #[error("Expected 'odd' or 'even' at {0}")]
    EvenOrOdd(usize),
    #[error("Missing plus sign at {0}")]
    MissingPlus(usize),
    #[error("Missing offset at {0}")]
    MissingOffset(usize),
}

impl From<OrdinalPatternError> for ParseError {
    fn from(val: OrdinalPatternError) -> Self {
        val.into_parse_error()
    }
}

impl OrdinalPatternError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for OrdinalPatternError {
    fn index(&self) -> usize {
        match *self {
            OrdinalPatternError::Invalid(index) => index,
            OrdinalPatternError::InvalidStep(index) => index,
            OrdinalPatternError::InvalidOffset(index) => index,
            OrdinalPatternError::Unterminated(index) => index,
            OrdinalPatternError::EvenOrOdd(index) => index,
            OrdinalPatternError::MissingPlus(index) => index,
            OrdinalPatternError::MissingOffset(index) => index,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum OrdinalPattern {
    Even,
    Odd,
    N(usize),
    Formula {
        backward: bool,
        a: usize,
        b: Option<usize>,
    },
}

impl Display for OrdinalPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrdinalPattern::Even => write!(f, "even"),
            OrdinalPattern::Odd => write!(f, "odd"),
            OrdinalPattern::N(n) => write!(f, "{}", n),
            OrdinalPattern::Formula { backward, a, b } => {
                if *backward {
                    write!(f, "-")?;
                }
                if *a > 1 {
                    write!(f, "{}", a)?;
                }
                write!(f, "n")?;
                if let Some(b) = b {
                    write!(f, "+{}", b)?;
                }
                Ok(())
            }
        }
    }
}

impl CssPattern<'_> for OrdinalPattern {
    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl OrdinalPattern {
    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        let Some((index, char)) = chars.current() else {
            debug!("No ordinal pattern found at {}", chars.last_index());
            return Ok(None);
        };

        if char == ')' {
            debug!("Empty ordinal pattern found at {}", index);
            return Ok(None);
        }

        if char.is_ascii_alphabetic() && !char.eq_ignore_ascii_case(&'n') {
            debug!("Ordinal pattern is even or odd at {}", index);
            let text_pattern = TextPattern::default()
                .allow_alphabetic()
                .start_with(CssChar::Alphabetic);
            let Some(text) = text_pattern
                .validate(chars)
                .context(OrdinalPatternError::EvenOrOdd(index))?
            else {
                return Err(OrdinalPatternError::EvenOrOdd(index).into());
            };
            if text.eq_ignore_ascii_case("odd") {
                return Ok(Some(Self::Odd));
            } else if text.eq_ignore_ascii_case("even") {
                return Ok(Some(Self::Even));
            } else {
                return Err(OrdinalPatternError::EvenOrOdd(index).into());
            }
        }

        let backward = if char == '-' {
            debug!("Set count direction to reverse at {}", index);
            chars.next();
            true
        } else {
            false
        };

        let mut nth = false;

        debug!("Parsing step at {}", chars.last_index());
        let a =
            IntegerPattern::from_chars(chars).context(OrdinalPatternError::InvalidStep(index))?;

        let Some((_, n_char)) = chars.current() else {
            if let Some(a) = a {
                debug!("Parsed offset without step at {}", chars.last_index());
                if backward {
                    return Err(OrdinalPatternError::Invalid(chars.last_index()).into());
                }
                return Ok(Some(Self::N(a)));
            } else {
                return Err(OrdinalPatternError::Unterminated(chars.last_index()).into());
            }
        };

        if n_char == ')' {
            debug!("Parsed offset only at {}", chars.last_index());
            if let Some(a) = a {
                if backward {
                    return Err(OrdinalPatternError::Invalid(chars.last_index()).into());
                }
                return Ok(Some(Self::N(a)));
            } else {
                return Err(OrdinalPatternError::Unterminated(chars.last_index()).into());
            }
        } else if n_char == 'n' {
            nth = true;
        } else {
            return Err(OrdinalPatternError::Invalid(chars.last_index()).into());
        }

        chars.skip_spaces();
        debug!("Parsing plus operator at {}", chars.last_index());
        let Some((plus_index, plus_char)) = chars.next() else {
            if backward {
                return Err(OrdinalPatternError::Invalid(chars.last_index()).into());
            }
            return Ok(Some(Self::Formula {
                backward,
                a: a.unwrap_or(1),
                b: None,
            }));
        };

        if plus_char == ')' {
            if backward {
                return Err(OrdinalPatternError::Invalid(chars.last_index()).into());
            }

            return Ok(Some(Self::Formula {
                backward,
                a: a.unwrap_or(1),
                b: None,
            }));
        }

        if plus_char != '+' {
            return Err(OrdinalPatternError::MissingPlus(plus_index).into());
        }

        debug!("Parsing offset at {}", chars.last_index());
        let Some((offset_index, _)) = chars.next() else {
            return Err(OrdinalPatternError::MissingOffset(chars.last_index()).into());
        };

        let Some(offset) = IntegerPattern::from_chars(chars)
            .context(OrdinalPatternError::InvalidOffset(offset_index))?
        else {
            return Err(OrdinalPatternError::MissingOffset(plus_index).into());
        };

        debug!(
            "Parsed step with variable and offset at {}",
            chars.last_index()
        );
        Ok(Some(Self::Formula {
            backward,
            a: a.unwrap_or(1),
            b: Some(offset),
        }))
    }

    /// https://developer.mozilla.org/en-US/docs/Web/CSS/:nth-child#functional_notation
    pub(crate) fn matches(&self, position: usize) -> bool {
        let (backward, step, offset) = match self {
            OrdinalPattern::Formula { backward, a, b } => (*backward, *a, b.unwrap_or(0)),
            OrdinalPattern::Even => (false, 2, 0),
            OrdinalPattern::Odd => (false, 2, 1),
            OrdinalPattern::N(p) => return *p == position,
        };

        logging::debug!(
            "position: {}, offset: {}, step: {}, backward: {}",
            position,
            offset,
            step,
            backward
        );

        let remainder;
        let quotient;

        if backward {
            // -an+b = position
            // -> n = (b - position) / a
            remainder = (offset as isize - position as isize) % step as isize;
            quotient = (offset as isize - position as isize) / step as isize;
        } else {
            // an+b = position
            // -> n = (position - b) / a
            remainder = (position as isize - offset as isize) % step as isize;
            quotient = (position as isize - offset as isize) / step as isize;
        }
        logging::debug!("r: {}", remainder);
        logging::debug!("q: {}", quotient);

        remainder == 0 && quotient >= 0
    }
}

#[test]
fn test_parse_ordinal_pattern() {
    use crate::css::{helpers::test_ok, patterns::integer_pattern::IntegerPatternError};

    test_ok("", None::<OrdinalPattern>);
    test_ok(
        "n",
        Some(OrdinalPattern::Formula {
            backward: false,
            a: 1,
            b: None,
        }),
    );
    test_ok("even", Some(OrdinalPattern::Even));
    test_ok("Odd", Some(OrdinalPattern::Odd));
    test_ok("2", Some(OrdinalPattern::N(2)));
    test_ok(
        "2n",
        Some(OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: None,
        }),
    );
    test_ok(
        "2n+4",
        Some(OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: Some(4),
        }),
    );
    test_ok(
        "-n+2",
        Some(OrdinalPattern::Formula {
            backward: true,
            a: 1,
            b: Some(2),
        }),
    );
    test_ok(
        "-3n+5",
        Some(OrdinalPattern::Formula {
            backward: true,
            a: 3,
            b: Some(5),
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<OrdinalPattern>(string, expected);
    }

    test_err("n+", OrdinalPatternError::MissingOffset(1).into());
    test_err(
        "18446744073709551616n",
        ParseError::Context(
            OrdinalPatternError::InvalidStep(0).into(),
            ParseError::from(IntegerPatternError::ParseFail(
                0,
                "number too large to fit in target type".to_string(),
            ))
            .into(),
        ),
    );
    test_err("+", OrdinalPatternError::Invalid(0).into());
    test_err("2+", OrdinalPatternError::Invalid(1).into());
    test_err("-3", OrdinalPatternError::Invalid(1).into());
    test_err("-4n", OrdinalPatternError::Invalid(2).into());
    test_err("2n-", OrdinalPatternError::MissingPlus(2).into());
    test_err("2n+", OrdinalPatternError::MissingOffset(2).into());
    test_err(
        "2n+18446744073709551616",
        ParseError::Context(
            OrdinalPatternError::InvalidOffset(3).into(),
            ParseError::from(IntegerPatternError::ParseFail(
                3,
                "number too large to fit in target type".to_string(),
            ))
            .into(),
        ),
    );
}

#[test]
fn test_ordinal_matching() {
    assert!(OrdinalPattern::Even.matches(0));
    assert!(!OrdinalPattern::Even.matches(1));
    assert!(OrdinalPattern::Even.matches(2));
    assert!(OrdinalPattern::Odd.matches(1));
    assert!(!OrdinalPattern::Odd.matches(2));
    assert!(OrdinalPattern::N(2).matches(2));
    assert!(!OrdinalPattern::N(2).matches(3));
    assert!(
        OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: None
        }
        .matches(2)
    );
    assert!(
        !OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: None
        }
        .matches(3)
    );
    assert!(
        OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: Some(4)
        }
        .matches(6)
    );
}
