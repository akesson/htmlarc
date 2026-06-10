use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeName, Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{AttributePattern, Brackets, CssPattern, Parenthesized},
    },
    dom::DomRead,
    html::HtmlElement,
    stores::{Attribute, Class, DataAttribute},
};

#[derive(Debug, Error)]
pub enum AttributeSelectorError {
    #[error("Failed to parse attribute selector at {0}")]
    ParseFail(usize),
    #[error("Empty brackets at {0}")]
    EmptyBrackets(usize),
}

impl From<AttributeSelectorError> for ParseError {
    fn from(val: AttributeSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl AttributeSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for AttributeSelectorError {
    fn index(&self) -> usize {
        match *self {
            AttributeSelectorError::ParseFail(index) => index,
            AttributeSelectorError::EmptyBrackets(index) => index,
        }
    }
}

/// See [mdn: Attribute selectors](https://developer.mozilla.org/en-US/docs/Web/CSS/Attribute_selectors)
#[derive(Debug, Clone)]
pub struct AttributeSelector<'s>(pub AttributePattern<'s>);

impl Display for AttributeSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]", self.0)
    }
}

impl<'s> CssPattern<'s> for AttributeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl PartialEq<Class<'_>> for AttributeSelector<'_> {
    fn eq(&self, other: &Class<'_>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Attribute<'_>> for AttributeSelector<'_> {
    fn eq(&self, other: &Attribute<'_>) -> bool {
        self.0 == *other
    }
}

impl PartialEq<DataAttribute<'_>> for AttributeSelector<'_> {
    fn eq(&self, other: &DataAttribute<'_>) -> bool {
        self.0 == *other
    }
}

impl<'s> AttributeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((i, _)) = chars.current() else {
            debug!("No attribute selector found at {}", chars.last_index());
            return Ok(None);
        };

        debug!("Parsing attribute pattern at {}", i);
        let pattern: Option<Parenthesized<Brackets, AttributePattern>> =
            Parenthesized::from_chars(chars).context(AttributeSelectorError::ParseFail(i))?;

        if let Some(parenthesized) = pattern {
            if let Some(attribute) = parenthesized.inner() {
                debug!("Parsed attribute selector at {}", i);
                Ok(Some(Self(attribute)))
            } else {
                Err(AttributeSelectorError::EmptyBrackets(i).into())
            }
        } else {
            debug!("Empty brackets found at {}", i);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_attribute_selector() {
    use crate::css::{
        helpers::test_ok,
        patterns::{
            AttributeOperator, AttributePatternError, AttributeValue, CaseIndicator,
            ParenthesizedError, QuotedString,
        },
    };
    use crate::html::HtmlAttr;

    test_ok("", None::<AttributeSelector>);
    test_ok(
        "[href]",
        Some(AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: None,
        })),
    );
    test_ok(
        "[src*=\".png\"]",
        Some(AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::src),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString(".png".into()),
                case: None,
            }),
        })),
    );
    test_ok(
        "[action=\"POST\" s]",
        Some(AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::action),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("POST".into()),
                case: Some(CaseIndicator::Sensitive),
            }),
        })),
    );
    test_ok(
        "[data-name]",
        Some(AttributeSelector(AttributePattern {
            name: AttributeName::Data("name"),
            value: None,
        })),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<AttributeSelector>(string, expected);
    }

    test_err("[]", AttributeSelectorError::EmptyBrackets(0).into());
    test_err(
        "[srt",
        ParseError::new(AttributePatternError::InvalidName(
            1,
            "Invalid HTML attribute: srt".to_string(),
        ))
        .context(ParenthesizedError::InvalidContent(1))
        .context(AttributeSelectorError::ParseFail(0)),
    );
    test_err(
        "[data-]",
        ParseError::new(AttributePatternError::InvalidName(
            1,
            "Invalid data attribute name".to_string(),
        ))
        .context(ParenthesizedError::InvalidContent(1))
        .context(AttributeSelectorError::ParseFail(0)),
    );
    test_err(
        "[src",
        ParseError::new(ParenthesizedError::MissingEndDelimiter(4, ']'))
            .context(AttributeSelectorError::ParseFail(0)),
    )
}
