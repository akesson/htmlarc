use std::fmt::Display;

use thiserror::Error;

use crate::css::{
    Context, IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug,
    patterns::CssPattern,
};

use super::{
    attribute_operator::AttributeOperator, case_indicator::CaseIndicator,
    quoted_string::QuotedString,
};

#[derive(Debug, Error)]
pub enum AttributeValueError {
    #[error("Invalid attribute operator at {0}")]
    InvalidOperator(usize),
    #[error("Invalid attribute value at {0}")]
    InvalidValue(usize),
    #[error("Invalid case indicator at {0}")]
    InvalidCase(usize),
    #[error("Missing attribute value at {0}")]
    MissingValue(usize),
}

impl From<AttributeValueError> for ParseError {
    fn from(value: AttributeValueError) -> Self {
        value.into_parse_error()
    }
}

impl AttributeValueError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for AttributeValueError {
    fn index(&self) -> usize {
        match *self {
            AttributeValueError::InvalidOperator(index) => index,
            AttributeValueError::InvalidValue(index) => index,
            AttributeValueError::InvalidCase(index) => index,
            AttributeValueError::MissingValue(index) => index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AttributeValue<'s> {
    pub operator: AttributeOperator,
    pub value: QuotedString<'s>,
    pub case: Option<CaseIndicator>,
}

impl Display for AttributeValue<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(case) = self.case.as_ref() {
            write!(f, "{}{}{}", self.operator, self.value, case)
        } else {
            write!(f, "{}{}", self.operator, self.value)
        }
    }
}

impl<'s> CssPattern<'s> for AttributeValue<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> AttributeValue<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((operator_index, _)) = chars.current() else {
            debug!("No attribute value found at {}", chars.last_index());
            return Ok(None);
        };

        debug!("Parsing attribute operator at {}", operator_index);
        if let Some(op) = AttributeOperator::from_chars(chars)
            .context(AttributeValueError::InvalidOperator(operator_index))?
        {
            let quote_index = chars.last_index();

            debug!("Parsing attribute string value at {}", quote_index);
            if let Some(val) = QuotedString::from_chars(chars)
                .context(AttributeValueError::InvalidValue(quote_index))?
            {
                let case_index = chars.last_index();
                debug!("Parsing case indicator at {}", case_index);
                let case = CaseIndicator::from_chars(chars)
                    .context(AttributeValueError::InvalidCase(case_index))?;

                Ok(Some(AttributeValue {
                    operator: op,
                    value: val,
                    case,
                }))
            } else {
                Err(AttributeValueError::MissingValue(chars.last_index()).into())
            }
        } else {
            debug!("No attribute operator found at {}", operator_index);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_attribute_value() {
    use crate::css::helpers::test_ok;

    test_ok(
        "=\"href\" i",
        Some(AttributeValue {
            operator: AttributeOperator::Exact,
            value: QuotedString("href".into()),
            case: Some(CaseIndicator::Insensitive),
        }),
    );
    test_ok(
        "*=\"url\"",
        Some(AttributeValue {
            operator: AttributeOperator::Includes,
            value: QuotedString("url".into()),
            case: None,
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<AttributeValue>(string, expected);
    }

    use crate::css::patterns::{
        attribute_operator::AttributeOperatorError, case_indicator::CaseIndicatorError,
        quoted_string::QuotedStringError,
    };

    test_err("=", AttributeValueError::MissingValue(0).into());
    test_err(
        "+",
        ParseError::Context(
            AttributeValueError::InvalidOperator(0).into(),
            ParseError::Parsing(AttributeOperatorError::InvalidAttributeOperator(0).into()).into(),
        ),
    );
    test_err(
        "=\"some",
        ParseError::Context(
            AttributeValueError::InvalidValue(1).into(),
            ParseError::Parsing(QuotedStringError::Unterminated(5).into()).into(),
        ),
    );
    test_err(
        "=\"text\" x",
        ParseError::Context(
            AttributeValueError::InvalidCase(7).into(),
            ParseError::Parsing(CaseIndicatorError::InvalidCaseIndicator(8).into()).into(),
        ),
    );
}
