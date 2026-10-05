use std::fmt::Display;

use thiserror::Error;
use tinyvec::TinyVec;

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
    stores::{AttrName, Attribute, Class},
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
    /// A standard attribute name.
    Std(HtmlAttr),
    /// Any other name — `data-*` or otherwise unrecognised — kept verbatim (full name).
    Ext(&'s str),
}

impl Display for AttributeName<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttributeName::Text => write!(f, "text"),
            AttributeName::Std(attr) => write!(f, "{attr}"),
            AttributeName::Ext(attr) => write!(f, "{attr}"),
        }
    }
}

impl<'s> TryFrom<&'s str> for AttributeName<'s> {
    type Error = AttributeNameError;

    fn try_from(value: &'s str) -> Result<Self, Self::Error> {
        // No name is rejected any more: a non-standard name is a valid extended-name
        // selector that matches an extended attribute (ADR 0002 §3). `text` stays special.
        Ok(if value == "text" {
            Self::Text
        } else {
            match HtmlAttr::try_from(value) {
                Ok(attr) => AttributeName::Std(attr),
                Err(_) => AttributeName::Ext(value),
            }
        })
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

/// Match a pattern against an attribute (ADR 0002 §3 — std, `data-*`, and unknown share one
/// store). The name must match; then the value (if the pattern has one). Values compare
/// case-sensitively, except the HTML standard's short list of ASCII-case-insensitive
/// attributes ([`HtmlAttr::is_value_case_insensitive`]: `type`, `lang`, `rel`, ...; and
/// [`HtmlAttr::is_ext_value_case_insensitive`] for the listed names stored as extended). An
/// explicit `s`/`i` flag overrides the default.
/// <https://html.spec.whatwg.org/multipage/semantics-other.html#case-sensitivity-of-selectors>
impl PartialEq<Attribute<'_>> for AttributePattern<'_> {
    fn eq(&self, other: &Attribute) -> bool {
        let insensitive_default = match (&self.name, &other.name) {
            (AttributeName::Std(p), AttrName::Std(a)) if p == a => a.is_value_case_insensitive(),
            (AttributeName::Ext(p), AttrName::Ext(n)) if p == n => {
                HtmlAttr::is_ext_value_case_insensitive(n)
            }
            _ => return false,
        };

        let Some(value) = &self.value else {
            return true;
        };
        let insensitive = match &value.case {
            Some(case) => *case == CaseIndicator::Insensitive,
            None => insensitive_default,
        };

        // The literal was entity-decoded once at parse (see QuotedString), so the match
        // compares decoded-vs-decoded with no work here.
        value
            .operator
            .matches(&value.value.0, other.val, insensitive)
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

    /// Match a `[class…]` pattern against an element's class list, as the CSS spec defines
    /// it: like any attribute, against the whole `class` value. Values are case-sensitive
    /// (`class` is not on the HTML case-insensitive list) unless the pattern has the `i` flag.
    /// <https://drafts.csswg.org/selectors-4/#attribute-representation>
    ///
    /// - `[class]` matches any element with a class attribute.
    /// - `[class~="foo"]` matches when one class is exactly `foo` (same as `.foo`). A value
    ///   that is empty or contains whitespace matches nothing.
    /// - `[class="foo bar"]`, `^=`, `$=`, `*=`, `|=` test the whole value. `[class^="foo"]`
    ///   matches `class="foobar baz"` but not `class="bar foo"`; `[class="foo"]` does not
    ///   match `class="foo bar"`.
    ///
    /// The class list is stored as its tokens, so the value tested is the tokens joined by
    /// single spaces — what you get from `" ".join(value.split())`, and what bs4 compares.
    /// It differs from the raw attribute only in whitespace: `class=" foo"` matches
    /// `[class^="foo"]`, and `class="a\tb"` or `class="a  b"` matches `[class="a b"]`.
    ///
    /// `spans_classes` says whether the value holds ASCII whitespace; the caller works it out
    /// once per selector, not per element.
    // Out of line: inlined into the select walk it measured ~4% slower on `[class*=]` and
    // ~8% slower on `[class~=]`/`[class^=]` (fr.serrer, interleaved A/B).
    #[inline(never)]
    pub(crate) fn matches_class_list<'c>(
        &self,
        mut classes: impl Iterator<Item = Class<'c>>,
        spans_classes: bool,
    ) -> bool {
        use AttributeOperator::*;
        if !matches!(self.name, AttributeName::Std(HtmlAttr::class)) {
            return false;
        }
        let Some(value) = &self.value else {
            return true;
        };
        let (op, p) = (value.operator, value.value.0.as_ref());
        let ci = value.case == Some(CaseIndicator::Insensitive);

        // A class never holds whitespace, so `~=` is an exact compare per class.
        if op == List {
            return !p.is_empty() && classes.any(|c| Exact.matches(p, c.0, ci));
        }
        if spans_classes {
            return spanning_match(op, p, classes, ci);
        }
        // Otherwise a match lies inside a single class, so test the class it would be in.
        if op == Includes {
            return classes.any(|c| op.matches(p, c.0, ci));
        }
        let Some(first) = classes.next() else {
            return false;
        };
        match op {
            Exact => op.matches(p, first.0, ci) && classes.next().is_none(),
            Starts => op.matches(p, first.0, ci),
            Ends => op.matches(p, classes.last().unwrap_or(first).0, ci),
            // `p` itself only matches when it is the whole value; `p-…` only needs the
            // first class.
            DashMatch => {
                op.matches(p, first.0, ci) && (first.0.len() > p.len() || classes.next().is_none())
            }
            List | Includes => unreachable!(),
        }
    }
}

/// Match a pattern that holds whitespace against the class list joined by single spaces,
/// without a heap allocation for any but very long lists.
fn spanning_match<'c>(
    op: AttributeOperator,
    p: &str,
    mut classes: impl Iterator<Item = Class<'c>>,
    ci: bool,
) -> bool {
    use AttributeOperator::*;
    // `=`, the common case: compare the pattern's space-separated parts class by class,
    // stopping at the first mismatch. A part that is empty or holds other whitespace
    // matches no class, just as the pattern would match no joined value.
    if op == Exact {
        let mut parts = p.split(' ');
        return classes.all(|c| parts.next().is_some_and(|w| Exact.matches(w, c.0, ci)))
            && parts.next().is_none();
    }
    let mut joined: TinyVec<[u8; 256]> = TinyVec::new();
    for (i, c) in classes.enumerate() {
        if i > 0 {
            joined.push(b' ');
        }
        joined.extend_from_slice(c.0.as_bytes());
    }
    // Classes joined by ASCII spaces are valid UTF-8.
    std::str::from_utf8(&joined).is_ok_and(|v| op.matches(p, v, ci))
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
            name: AttributeName::Std(HtmlAttr::src),
            value: None,
        }),
    );
    test_ok(
        "data-name",
        Some(AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: None,
        }),
    );
    test_ok(
        "data-name=\"custom\"",
        Some(AttributePattern {
            name: AttributeName::Ext("data-name"),
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
            name: AttributeName::Std(HtmlAttr::href),
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
            name: AttributeName::Std(HtmlAttr::src),
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

    // `data-` is no longer rejected — it parses to an extended-name pattern (ADR 0002 §3).
    test_ok(
        "data-",
        Some(AttributePattern {
            name: AttributeName::Ext("data-"),
            value: None,
        }),
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
            name: AttributeName::Ext("data-name"),
            value: None,
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Sensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("Cus".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("uS".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "CuStom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("oM".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "CustoM",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "test Custom foo bar",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: None,
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom-foo",
        }
    );
}

#[test]
fn test_data_attribute_matching_insensitive() {
    use super::*;

    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("Cus".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("uS".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("oM".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "Custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "test custom foo bar",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "custom",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("Custom".into()),
                case: Some(CaseIndicator::Insensitive),
            }),
        },
        Attribute {
            name: AttrName::Ext("data-name"),
            val: "custom-foo",
        }
    );
}

/// `matches_class_list` against `classes`, with `pattern` like `^="cus" i`.
#[cfg(test)]
fn class_list_matches(
    operator: AttributeOperator,
    value: &str,
    ci: bool,
    classes: &[&str],
) -> bool {
    crate::css::AttributeSelector::new(AttributePattern {
        name: AttributeName::Std(HtmlAttr::class),
        value: Some(AttributeValue {
            operator,
            value: QuotedString(value.into()),
            case: ci.then_some(CaseIndicator::Insensitive),
        }),
    })
    .matches_class_list(classes.iter().map(|c| Class(c)))
}

#[test]
fn test_class_matching_whole_value() {
    use AttributeOperator::*;

    let bare = AttributePattern {
        name: AttributeName::Std(HtmlAttr::class),
        value: None,
    };
    assert!(bare.matches_class_list([Class("custom")].into_iter(), false));

    let cases: &[(AttributeOperator, &str, &[&str], bool)] = &[
        (Exact, "Custom", &["Custom"], true),
        (Exact, "Custom", &["Custom", "foo"], false),
        (Exact, "Custom foo", &["Custom", "foo"], true),
        (Exact, "", &[""], true),
        (Starts, "Cus", &["Custom", "foo"], true),
        (Starts, "Cus", &["foo", "Custom"], false),
        (Starts, "Custom f", &["Custom", "foo"], true),
        (Starts, "", &["Custom"], false),
        (Ends, "Om", &["foo", "CustOm"], true),
        (Ends, "Om", &["CustOm", "foo"], false),
        (Ends, "", &["Custom"], false),
        (Includes, "Us", &["foo", "CUstom"], true),
        (Includes, "m f", &["Custom", "foo"], true),
        (Includes, "", &["Custom"], false),
        (List, "CusTom", &["foo", "CusTom"], true),
        (List, "sTo", &["CusTom"], false),
        (List, "a b", &["a", "b"], false),
        (List, "", &[""], false),
        (DashMatch, "Custom", &["Custom"], true),
        (DashMatch, "Custom", &["Custom-foo", "bar"], true),
        (DashMatch, "Custom", &["Custom", "bar"], false),
        (DashMatch, "Custom", &["Custombar"], false),
        (DashMatch, "", &["-x", "y"], true),
    ];
    for &(op, value, classes, expected) in cases {
        assert_eq!(
            class_list_matches(op, value, false, classes),
            expected,
            "[class{op}{value:?}] vs {classes:?}"
        );
    }
}

#[test]
fn test_class_matching_case() {
    use AttributeOperator::*;

    // Case-sensitive by default; the `i` flag folds ASCII case.
    for (op, value) in [
        (Exact, "custom"),
        (Starts, "cus"),
        (Includes, "us"),
        (Ends, "om"),
        (List, "custom"),
        (DashMatch, "custom"),
    ] {
        assert!(
            !class_list_matches(op, value, false, &["CUSTOM"]),
            "[class{op}{value:?}]"
        );
        assert!(
            class_list_matches(op, value, true, &["CUSTOM"]),
            "[class{op}{value:?} i]"
        );
    }
}

#[test]
fn test_attribute_matching() {
    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::width),
            value: None
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::width),
            val: "",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("http".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("httP".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("http".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("httP".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tt".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tT".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tt".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString("tT".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http://",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("tp".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("Tp".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::href),
            val: "http",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("en-Us".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::lang),
            val: "fr-Fr en-Us",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::List,
                value: QuotedString("en-us".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::lang),
            val: "fr-Fr en-Us",
        }
    );

    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::lang),
            val: "en",
        }
    );
    assert_eq!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: None
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::lang),
            val: "en-Us",
        }
    );
    assert_ne!(
        AttributePattern {
            name: AttributeName::Std(HtmlAttr::lang),
            value: Some(AttributeValue {
                operator: AttributeOperator::DashMatch,
                value: QuotedString("en".into()),
                case: Some(CaseIndicator::Sensitive)
            }),
        },
        Attribute {
            name: AttrName::Std(HtmlAttr::lang),
            val: "EN-US",
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
                name: AttributeName::Std(attr),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Exact,
                    value: QuotedString("Test".into()),
                    case: None
                })
            },
            Attribute {
                name: AttrName::Std(attr),
                val: "Test"
            }
        );
        assert_ne!(
            AttributePattern {
                name: AttributeName::Std(attr),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Exact,
                    value: QuotedString("Test".into()),
                    case: None
                })
            },
            Attribute {
                name: AttrName::Std(attr),
                val: "test"
            }
        );
    }
}

#[test]
fn test_default_value_case_follows_the_html_list() {
    let pattern = |attr, op, value: &'static str| AttributePattern {
        name: AttributeName::Std(attr),
        value: Some(AttributeValue {
            operator: op,
            value: QuotedString(value.into()),
            case: None,
        }),
    };
    let attribute = |attr, val| Attribute {
        name: AttrName::Std(attr),
        val,
    };
    // `href` is not on the HTML standard's list: case-sensitive, like browsers.
    let http = pattern(HtmlAttr::href, AttributeOperator::Starts, "http://");
    assert_eq!(http, attribute(HtmlAttr::href, "http://a.example/"));
    assert_ne!(http, attribute(HtmlAttr::href, "HTTP://a.example/"));
    // `type` is on it: ASCII case-insensitive for every operator.
    for (op, val) in [
        (AttributeOperator::Exact, "TEXT/JavaScript"),
        (AttributeOperator::Starts, "TEXT/js"),
        (AttributeOperator::Ends, "x/JAVASCRIPT"),
        (AttributeOperator::Includes, "a/JavaScript+b"),
        (AttributeOperator::List, "a TEXT/JAVASCRIPT"),
        (AttributeOperator::DashMatch, "TEXT/javascript-x"),
    ] {
        let needle = match op {
            AttributeOperator::Starts => "text/",
            AttributeOperator::Ends | AttributeOperator::Includes => "javascript",
            _ => "text/javascript",
        };
        assert_eq!(
            pattern(HtmlAttr::type_, op, needle),
            attribute(HtmlAttr::type_, val),
            "{op:?} {val}"
        );
    }
    // ...and only ASCII folds: `É` and `é` stay different.
    assert_ne!(
        pattern(HtmlAttr::lang, AttributeOperator::Exact, "é"),
        attribute(HtmlAttr::lang, "É")
    );
    // A dash-match needs the dash right after the prefix.
    let en = pattern(HtmlAttr::lang, AttributeOperator::DashMatch, "en");
    assert_eq!(en, attribute(HtmlAttr::lang, "EN-us"));
    assert_ne!(en, attribute(HtmlAttr::lang, "eng"));
}

#[test]
fn test_spec_list_covers_extended_names() {
    let pattern = |value: &'static str| AttributePattern {
        name: AttributeName::Ext("language"),
        value: Some(AttributeValue {
            operator: AttributeOperator::Exact,
            value: QuotedString(value.into()),
            case: None,
        }),
    };
    let attribute = |name, val| Attribute {
        name: AttrName::Ext(name),
        val,
    };
    // `language` is on the HTML standard's list but not an `HtmlAttr`: still case-insensitive.
    assert_eq!(pattern("javascript"), attribute("language", "JavaScript"));
    // Any other extended name (here `data-mode`) stays case-sensitive.
    let data = AttributePattern {
        name: AttributeName::Ext("data-mode"),
        ..pattern("dark")
    };
    assert_ne!(data, attribute("data-mode", "Dark"));
}
