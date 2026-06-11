use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeName, Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{AttributePattern, Brackets, CssPattern, Parenthesized},
        selectors::ResolvedRef,
    },
    dom::{DomRead, DomView},
    html::HtmlElement,
    stores::{Attribute, Class, NAME_EXT_BASE},
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
pub struct AttributeSelector<'s> {
    pub pattern: AttributePattern<'s>,
    /// The resolved `NameSym` of the pattern's name (ADR 0002 §3): an integer prefilter for
    /// per-node matching, or `Absent` when an extended name is not in the document.
    pub(crate) resolved: ResolvedRef,
}

impl<'s> AttributeSelector<'s> {
    pub fn new(pattern: AttributePattern<'s>) -> Self {
        Self {
            pattern,
            resolved: ResolvedRef::Unresolved,
        }
    }

    /// Bind this selector to a document by resolving its attribute *name* to a `NameSym`: a
    /// standard name is its constant `HtmlAttr` repr, an extended name resolves through the
    /// document symbol table (`Absent` when missing). The value comparison stays in
    /// [`AttributePattern`]'s `PartialEq` — the name resolution is the per-node integer
    /// prefilter and the absent-name prune.
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        self.resolved = match &self.pattern.name {
            AttributeName::Std(attr) => ResolvedRef::Found(*attr as u16),
            AttributeName::Ext(name) => match view.symbols.find(name) {
                Some(sym) => ResolvedRef::Found(sym.as_u16() + NAME_EXT_BASE),
                None => ResolvedRef::Absent,
            },
            // `[text]` selectors are routed to `CompoundSelector::text`, never here.
            AttributeName::Text => ResolvedRef::Unresolved,
        };
    }
}

impl Display for AttributeSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}]", self.pattern)
    }
}

impl<'s> CssPattern<'s> for AttributeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl PartialEq<Class<'_>> for AttributeSelector<'_> {
    fn eq(&self, other: &Class<'_>) -> bool {
        self.pattern == *other
    }
}

impl PartialEq<Attribute<'_>> for AttributeSelector<'_> {
    fn eq(&self, other: &Attribute<'_>) -> bool {
        self.pattern == *other
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
                Ok(Some(Self::new(attribute)))
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
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::href),
            value: None,
        })),
    );
    test_ok(
        "[src*=\".png\"]",
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::src),
            value: Some(AttributeValue {
                operator: AttributeOperator::Includes,
                value: QuotedString(".png".into()),
                case: None,
            }),
        })),
    );
    test_ok(
        "[action=\"POST\" s]",
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::action),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("POST".into()),
                case: Some(CaseIndicator::Sensitive),
            }),
        })),
    );
    test_ok(
        "[data-name]",
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Ext("data-name"),
            value: None,
        })),
    );
    // An unknown name is no longer a parse error: it is a valid extended-name selector
    // (ADR 0002 §3). `[srt]` and `[data-]` parse to `Ext` patterns.
    test_ok(
        "[srt]",
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Ext("srt"),
            value: None,
        })),
    );
    test_ok(
        "[data-]",
        Some(AttributeSelector::new(AttributePattern {
            name: AttributeName::Ext("data-"),
            value: None,
        })),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<AttributeSelector>(string, expected);
    }

    test_err("[]", AttributeSelectorError::EmptyBrackets(0).into());
    // Missing the closing bracket is still an error (the name parses, the bracket doesn't).
    test_err(
        "[srt",
        ParseError::new(ParenthesizedError::MissingEndDelimiter(4, ']'))
            .context(AttributeSelectorError::ParseFail(0)),
    );
    test_err(
        "[src",
        ParseError::new(ParenthesizedError::MissingEndDelimiter(4, ']'))
            .context(AttributeSelectorError::ParseFail(0)),
    )
}
