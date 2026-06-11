use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult, chars::CssChars, logging::debug,
        patterns::CssPattern,
    },
    dom::DomRead,
    html::HtmlElement,
};

use super::{Selector, complex_relative::ComplexRelativeSelector};
use crate::dom::DomView;

impl RelativeSelectorList<'_> {
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        for selector in &mut self.selectors {
            selector.resolve(view);
        }
    }
}

#[derive(Debug, Error)]
pub enum RelativeSelectorListError {
    #[error("Failed to parse relative selector list at {0}")]
    ParseFail(usize),
    #[error("Unterminated relative selector list at {0}")]
    Unterminated(usize),
}

impl From<RelativeSelectorListError> for ParseError {
    fn from(val: RelativeSelectorListError) -> Self {
        val.into_parse_error()
    }
}

impl RelativeSelectorListError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for RelativeSelectorListError {
    fn index(&self) -> usize {
        match *self {
            RelativeSelectorListError::ParseFail(index) => index,
            RelativeSelectorListError::Unterminated(index) => index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelativeSelectorList<'s> {
    pub selectors: Vec<ComplexRelativeSelector<'s>>,
}

impl Display for RelativeSelectorList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.selectors
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl<'s> CssPattern<'s> for RelativeSelectorList<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for RelativeSelectorList<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> RelativeSelectorList<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((index, _)) = chars.current() else {
            debug!("No relative selector list found at {}", chars.last_index());
            return Ok(None);
        };

        let mut selectors = Vec::new();

        let mut expect_selector = true;

        while let Some(selector) = ComplexRelativeSelector::from_chars(chars)
            .context(RelativeSelectorListError::ParseFail(index))?
        {
            selectors.push(selector);
            chars.skip_spaces();
            expect_selector = false;
            debug!(
                "Adding complex relative selector to list at {}",
                chars.last_index()
            );

            if let Some((_, c)) = chars.current() {
                if c == ',' {
                    expect_selector = true;
                    chars.next();
                    chars.skip_spaces();
                } else {
                    break;
                }
            }
        }

        if expect_selector {
            return Err(RelativeSelectorListError::Unterminated(chars.last_index()).into());
        }

        debug!("Parsed relative selector list at {}", chars.last_index());
        Ok(Some(Self { selectors }))
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        for selector in &self.selectors {
            if selector.matches(el) {
                return true;
            }
        }

        false
    }
}

#[test]
fn test_parse_relative_selector_list() {
    use crate::{
        css::{
            helpers::test_ok,
            patterns::Combinator,
            selectors::{
                complex_relative::ComplexRelativeSelectorError,
                compound::{CompoundSelector, CompoundSelectorError},
                relative::{RelativeSelector, RelativeSelectorError},
            },
        },
        html::HtmlTag,
    };

    test_ok("", None::<RelativeSelectorList>);
    test_ok(
        "p",
        Some(RelativeSelectorList {
            selectors: vec![ComplexRelativeSelector {
                selectors: vec![RelativeSelector {
                    combinator: Combinator::Descendant,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::p),
                        ..Default::default()
                    },
                }],
            }],
        }),
    );
    test_ok(
        "p, +h1",
        Some(RelativeSelectorList {
            selectors: vec![
                ComplexRelativeSelector {
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::Descendant,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::p),
                            ..Default::default()
                        },
                    }],
                },
                ComplexRelativeSelector {
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::NextSibling,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::h1),
                            ..Default::default()
                        },
                    }],
                },
            ],
        }),
    );
    test_ok(
        "+ h1 > span, +h2 > i",
        Some(RelativeSelectorList {
            selectors: vec![
                ComplexRelativeSelector {
                    selectors: vec![
                        RelativeSelector {
                            combinator: Combinator::NextSibling,
                            selector: CompoundSelector {
                                element: Some(HtmlTag::h1),
                                ..Default::default()
                            },
                        },
                        RelativeSelector {
                            combinator: Combinator::Child,
                            selector: CompoundSelector {
                                element: Some(HtmlTag::span),
                                ..Default::default()
                            },
                        },
                    ],
                },
                ComplexRelativeSelector {
                    selectors: vec![
                        RelativeSelector {
                            combinator: Combinator::NextSibling,
                            selector: CompoundSelector {
                                element: Some(HtmlTag::h2),
                                ..Default::default()
                            },
                        },
                        RelativeSelector {
                            combinator: Combinator::Child,
                            selector: CompoundSelector {
                                element: Some(HtmlTag::i),
                                ..Default::default()
                            },
                        },
                    ],
                },
            ],
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<RelativeSelectorList>(string, expected);
    }

    test_err("+div,", RelativeSelectorListError::Unterminated(4).into());
    test_err(
        "+div, [src]img",
        ParseError::Context(
            RelativeSelectorListError::ParseFail(0).into(),
            ParseError::Context(
                ComplexRelativeSelectorError::ParseFail(6).into(),
                ParseError::Context(
                    RelativeSelectorError::SelectorFail(6).into(),
                    ParseError::from(CompoundSelectorError::UnexpectedChar(11)).into(),
                )
                .into(),
            )
            .into(),
        ),
    );
}
