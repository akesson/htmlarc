use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeOperator, CaseIndicator, Context, IndexedError, ParseError, ParseResult,
        QuotedString,
        chars::CssChars,
        ext::OptionExt,
        logging::debug,
        patterns::{CssPattern, text_pattern::TextPattern},
    },
    html::HtmlAttr,
    stores::{Attribute, Class, ClassList, DataAttribute},
};

use super::{attribute_value::AttributeValue, text_pattern::CssChar};

#[derive(Debug, Error)]
pub enum AttributePatternError {
    #[error("Invalid attribute name at {0}: {1}")]
    InvalidName(usize, String),
    #[error("Failed to parse attribute name at {0}")]
    ParseName(usize),
    #[error("Failed to parse attribute value at {0}")]
    ParseValue(usize),
}

impl From<AttributePatternError> for ParseError {
    fn from(val: AttributePatternError) -> Self {
        val.into_parse_error()
    }
}

impl AttributePatternError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for AttributePatternError {
    fn index(&self) -> usize {
        match *self {
            AttributePatternError::InvalidName(index, _) => index,
            AttributePatternError::ParseName(index) => index,
            AttributePatternError::ParseValue(index) => index,
        }
    }
}

#[derive(Debug, Error)]
pub enum AttributeNameError {
    #[error("Invalid data attribute name")]
    InvalidDataName,
    #[error("Invalid HTML attribute: {0}")]
    InvalidHtmlAttr(String),
}

#[derive(Debug, Clone)]
pub enum AttributeName<'s> {
    Text,
    Html(HtmlAttr),
    Data(&'s str),
}

impl Display for AttributeName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributeName::Text => write!(f, "text"),
            AttributeName::Html(attr) => write!(f, "{}", attr),
            AttributeName::Data(attr) => write!(f, "data-{}", attr),
        }
    }
}

impl<'s> TryFrom<&'s str> for AttributeName<'s> {
    type Error = AttributeNameError;

    fn try_from(value: &'s str) -> Result<Self, Self::Error> {
        if value == "text" {
            Ok(Self::Text)
        } else if let Some(name) = value.strip_prefix("data-") {
            if name.is_empty() {
                Err(AttributeNameError::InvalidDataName)
            } else {
                Ok(AttributeName::Data(name))
            }
        } else {
            HtmlAttr::try_from(value)
                .map(AttributeName::Html)
                .map_err(|_| AttributeNameError::InvalidHtmlAttr(value.to_string()))
        }
    }
}

#[derive(Debug, Clone)]
pub struct AttributePattern<'s> {
    pub name: AttributeName<'s>,
    pub value: Option<AttributeValue<'s>>,
}

impl Display for AttributePattern<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(value) = self.value.as_ref() {
            write!(f, "{}{}", self.name, value)
        } else {
            write!(f, "{}", self.name)
        }
    }
}

/// By default, data attributes are case-insensitive.
/// https://developer.mozilla.org/en-US/docs/Web/CSS/Attribute_selectors#description
impl PartialEq<DataAttribute<'_>> for AttributePattern<'_> {
    fn eq(&self, data_attribute: &DataAttribute) -> bool {
        if let AttributeName::Data(name) = self.name {
            if name != data_attribute.tag {
                return false;
            }

            if let Some(value) = &self.value {
                return if let Some(CaseIndicator::Insensitive) = &value.case {
                    let search = value.value.0.to_lowercase();
                    let other = data_attribute.val.to_lowercase();

                    value.operator.matches(&search, &other)
                } else {
                    value.operator.matches(&value.value.0, data_attribute.val)
                };
            }

            true
        } else {
            false
        }
    }
}

impl PartialEq<Class<'_>> for AttributePattern<'_> {
    fn eq(&self, other: &Class) -> bool {
        self.eq_class(other)
    }
}

impl PartialEq<Attribute<'_>> for AttributePattern<'_> {
    fn eq(&self, other: &Attribute) -> bool {
        if let AttributeName::Html(attr) = self.name {
            if attr != other.tag {
                return false;
            }

            let mut insensitive = !attr.is_case_sensitive();

            if let Some(value) = &self.value {
                if let Some(case) = &value.case {
                    insensitive = *case == CaseIndicator::Insensitive;
                }

                // The literal was entity-decoded once at parse (see QuotedString), so the
                // match compares decoded-vs-decoded with no work here.
                return if insensitive {
                    let search = value.value.0.to_lowercase();
                    let other = other.val.to_lowercase();

                    value.operator.matches(&search, &other)
                } else {
                    value.operator.matches(&value.value.0, other.val)
                };
            }

            true
        } else {
            false
        }
    }
}

impl<'s> CssPattern<'s> for AttributePattern<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> AttributePattern<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((start_index, _)) = chars.current() else {
            debug!("No attribute pattern found at {}", chars.last_index());
            return Ok(None);
        };

        let attribute_pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Alphabetic)
            .allow_special('-')
            .allow_special('_')
            .allow_special(':')
            .allow_special('.')
            .not_exclusively(CssChar::Digit)
            .not_exclusively(CssChar::Special('-'))
            .not_exclusively(CssChar::Special('_'))
            .not_exclusively(CssChar::Special(':'))
            .not_exclusively(CssChar::Special('.'))
            .stop_at(']');

        debug!("Parsing attribute name at {}", start_index);
        if let Some(attribute_name) = attribute_pattern
            .validate(chars)
            .context(AttributePatternError::ParseName(start_index))?
        {
            debug!("Attribute name: {}", attribute_name);

            match AttributeName::try_from(attribute_name) {
                Ok(attr) => {
                    let Some((value_index, _)) = chars.current() else {
                        return Ok(Some(Self {
                            name: attr,
                            value: None,
                        }));
                    };

                    debug!("Parsing attribute value at {}", value_index);
                    let value = AttributeValue::from_chars(chars)
                        .context(AttributePatternError::ParseValue(value_index))?;

                    debug!("Parsed attribute pattern '{}{}'", attr, value.string());
                    Ok(Some(Self { name: attr, value }))
                }
                Err(e) => {
                    Err(AttributePatternError::InvalidName(start_index, e.to_string()).into())
                }
            }
        } else {
            debug!("No attribute name found at {}", start_index);
            Ok(None)
        }
    }

    /// By default, class attributes are case-sensitive. <br>
    /// https://developer.mozilla.org/en-US/docs/Web/CSS/Attribute_selectors#description
    ///
    /// # Note
    /// This implementation doesn't follow the CSS spec for class matching.
    /// It checks each class inside the class attribute instead of checking the class attribute as a whole
    ///
    /// # Description
    /// - `[class]` -> matches any element with a class attribute
    /// ```html
    /// <!-- matches -->
    /// <div class="custom">...</div>
    /// ```
    ///
    /// - `[class="custom"]` -> matches any element that has the class "custom"
    /// ```html
    /// <!-- matches -->
    /// <div class="custom">...</div>
    /// <div class="bar custom foo">...</div>
    /// ```
    /// - `[class^="cus"]` -> matches any element with a class that starts with "cus"
    /// ```html
    /// <!-- matches -->
    /// <div class="foo custom">...</div>
    /// ```
    /// - `[class$="tom"]` -> matches any element with a class that ends with "tom"
    /// ```html
    /// <!-- matches -->
    /// <div class="custom foo">...</div>
    /// ```
    /// - `[class*="sto"]` -> matches any element with a class that includes "sto"
    /// ```html
    /// <!-- matches -->
    /// <div class="custom foo">...</div>
    /// ```
    /// - `[class~="sto"]` -> matches any element with a class that includes "sto"
    /// ```html
    /// <!-- matches -->
    /// <div class="foo custom bar">...</div>
    /// ```
    /// - `[class|="custom"]` -> matches any element with a class that starts with "custom-" or is "custom"
    /// ```html
    /// <!-- matches -->
    /// <div class="foo custom">...</div>
    /// <div class="foo custom-bar">...</div>
    /// <!-- doesn't match -->
    /// <div class="foo custombar">...</div>
    fn eq_class(&self, other: &Class) -> bool {
        if let AttributeName::Html(HtmlAttr::class) = self.name {
            let Some(value) = &self.value else {
                return true;
            };

            let (pattern, other) = if let Some(CaseIndicator::Insensitive) = &value.case {
                (value.value.0.to_lowercase(), other.0.to_lowercase())
            } else {
                (value.value.0.to_string(), other.0.to_string())
            };

            if value.operator == AttributeOperator::List {
                return value.operator.matches_includes(&pattern, &other);
            }

            return value.operator.matches(&pattern, &other);
        }

        false
    }
}

#[test]
fn test_parse_attribute_pattern() {
    use crate::css::{
        helpers::test_ok,
        patterns::{
            attribute_operator::AttributeOperator, attribute_value::AttributeValueError,
            case_indicator::CaseIndicator, quoted_string::QuotedString,
            text_pattern::TextPatternError,
        },
    };

    test_ok("", None::<AttributePattern>);
    test_ok("]", None::<AttributePattern>);
    test_ok(
        "text",
        Some(AttributePattern {
            name: AttributeName::Text,
            value: None,
        }),
    );
    test_ok(
        "src",
        Some(AttributePattern {
            name: AttributeName::Html(HtmlAttr::src),
            value: None,
        }),
    );
    test_ok(
        "data-name",
        Some(AttributePattern {
            name: AttributeName::Data("name"),
            value: None,
        }),
    );
    test_ok(
        "data-name=\"custom\"",
        Some(AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("custom".into()),
                case: None,
            }),
        }),
    );
    test_ok(
        "href^=\"https://\" s",
        Some(AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("https://".into()),
                case: Some(CaseIndicator::Sensitive),
            }),
        }),
    );
    test_ok(
        "src='image'",
        Some(AttributePattern {
            name: AttributeName::Html(HtmlAttr::src),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("image".into()),
                case: None,
            }),
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<AttributePattern>(string, expected);
    }

    test_err(
        ":",
        ParseError::Context(
            AttributePatternError::ParseName(0).into(),
            ParseError::from(TextPatternError::StartsWith(0, ':')).into(),
        ),
    );

    test_err(
        "data-",
        ParseError::new(AttributePatternError::InvalidName(
            0,
            "Invalid data attribute name".to_string(),
        )),
    );

    test_err(
        "src=",
        ParseError::Context(
            AttributePatternError::ParseValue(3).into(),
            ParseError::from(AttributeValueError::MissingValue(3)).into(),
        ),
    );
}

#[test]
fn test_data_attribute_matching_sensitive() {
    use super::*;

    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: None,
        },
        DataAttribute {
            tag: "name",
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Sensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("Cus".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("uS".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "CuStom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("oM".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "CustoM",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "test Custom foo bar",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom-foo",
        }
    );
}

#[test]
fn test_data_attribute_matching_insensitive() {
    use super::*;

    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("Cus".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("uS".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("oM".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "test custom foo bar",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Data("name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        DataAttribute {
            tag: "name",
            val: "custom-foo",
        }
    );
}

#[test]
fn test_class_matching_sensitive() {
    use super::*;

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: None,
        },
        Class("custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: None
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("Cus".into()),
                case: None
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("Us".into()),
                case: None
            }),
        },
        Class("CUstom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("Om".into()),
                case: None
            }),
        },
        Class("CustOm")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("sTo".into()),
                case: None
            }),
        },
        Class("CusTom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None
            }),
        },
        Class("Custom-foo")
    );
}

#[test]
fn test_class_matching_insensitive() {
    use super::*;

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("custom".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("cus".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("us".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("CUstom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("om".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("CustOm")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("sto".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("CUSTOM")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("custom".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("Custom")
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("custom".into()),
                case: Some(CaseIndicator::Insensitive)
            }),
        },
        Class("Custom-foo")
    );
}

#[test]
fn test_attribute_matching() {
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::width),
            value: None
        },
        Attribute {
            tag: HtmlAttr::width,
            val: ""
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("http".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("httP".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("http".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("httP".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tt".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tT".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tt".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tT".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http://"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("tp".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("Tp".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::href,
            val: "http"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("en-Us".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::lang,
            val: "fr-Fr en-Us"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("en-us".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::lang,
            val: "fr-Fr en-Us"
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::lang,
            val: "en"
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: None
            }),
        },
        Attribute {
            tag: HtmlAttr::lang,
            val: "en-Us"
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Html(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            tag: HtmlAttr::lang,
            val: "EN-US"
        }
    );
}

#[test]
fn test_sensitive_attributes_matching() {
    const CASE_SENSITIVE_ATTRIBUTES: [HtmlAttr; 9] = [
        HtmlAttr::id,
        HtmlAttr::aria_controls,
        HtmlAttr::aria_expanded,
        HtmlAttr::aria_haspopup,
        HtmlAttr::aria_hidden,
        HtmlAttr::aria_label,
        HtmlAttr::aria_labelledby,
        HtmlAttr::aria_pressed,
        HtmlAttr::role,
    ];

    for attr in CASE_SENSITIVE_ATTRIBUTES {
        assert_eq!(
            AttributePattern {
                name: AttributeName::Html(attr),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Exact,
                    value: QuotedString("Test".into()),
                    case: None
                })
            },
            Attribute {
                tag: attr,
                val: "Test"
            }
        );
        assert_ne!(
            AttributePattern {
                name: AttributeName::Html(attr),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Exact,
                    value: QuotedString("Test".into()),
                    case: None
                })
            },
            Attribute {
                tag: attr,
                val: "test"
            }
        );
    }
}
