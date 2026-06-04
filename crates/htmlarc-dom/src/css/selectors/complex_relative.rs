use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Combinator, Context, IndexedError, ParseError, ParseResult, Selector,
        chars::CssChars,
        logging::{self, debug},
        patterns::CssPattern,
    },
    dom::DomRead,
    html::{HtmlElement, HtmlTag, IGNORE_TAGS},
    iters::RelativeIter,
};

use super::relative::RelativeSelector;

#[derive(Debug, Error)]
pub enum ComplexRelativeSelectorError {
    #[error("Failed to parse complex relative selector at {0}")]
    ParseFail(usize),
}

impl From<ComplexRelativeSelectorError> for ParseError {
    fn from(val: ComplexRelativeSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl ComplexRelativeSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for ComplexRelativeSelectorError {
    fn index(&self) -> usize {
        match *self {
            ComplexRelativeSelectorError::ParseFail(index) => index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ComplexRelativeSelector<'s> {
    pub selectors: Vec<RelativeSelector<'s>>,
}

impl Display for ComplexRelativeSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for selector in &self.selectors {
            if selector.combinator == Combinator::Descendant {
                if first {
                    first = false;
                    write!(f, "{}", selector.selector)?;
                } else {
                    write!(f, "{}", selector)?;
                }
            } else {
                write!(f, " {} {}", selector.combinator, selector.selector)?;
            }
        }

        Ok(())
    }
}

impl<'s> CssPattern<'s> for ComplexRelativeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for ComplexRelativeSelector<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> ComplexRelativeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((index, _)) = chars.current() else {
            debug!(
                "No complex relative selector found at {}",
                chars.last_index()
            );
            return Ok(None);
        };

        let mut selectors = Vec::new();

        while let Some(selector) = RelativeSelector::from_chars(chars)
            .context(ComplexRelativeSelectorError::ParseFail(index))?
        {
            selectors.push(selector);
        }

        debug!("Parsed complex relative selector at {}", chars.last_index());
        Ok(Some(Self { selectors }))
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        Self::verify(&self.selectors, 0, el)
    }

    fn verify(
        selectors: &[RelativeSelector],
        cursor: isize,
        el: &HtmlElement<impl DomRead>,
    ) -> bool {
        if cursor == selectors.len() as isize {
            return true;
        }

        let selector = &selectors[cursor as usize];
        debug!("Checking selector: {}", selector);

        match selector.combinator {
            Combinator::Descendant => {
                debug!("Searching descendants of {}", el.tag());
                let descendants = el.descendants().filter(|e| selector.matches(e));

                for descendant in descendants {
                    debug!("Checking descendant: {}", descendant.tag());
                    if Self::verify(selectors, cursor + 1, &descendant) {
                        return true;
                    }
                }

                false
            }
            Combinator::Child => {
                debug!("Searching children of {}", el.tag());
                let children = RelativeIter::children(el).filter(|el| selector.matches(el));

                for child in children {
                    debug!("Checking child: {}", child.tag());
                    if Self::verify(selectors, cursor + 1, &child) {
                        return true;
                    }
                }

                false
            }
            Combinator::SubsequentSibling => {
                debug!("Searching subsequent siblings of {}", el.tag());
                let siblings = RelativeIter::next_siblings(el).filter(|el| selector.matches(el));

                for sibling in siblings {
                    debug!("Checking sibling: {}", sibling.tag());
                    if Self::verify(selectors, cursor + 1, &sibling) {
                        return true;
                    }
                }

                false
            }
            Combinator::NextSibling => {
                debug!("Searching next sibling of {}", el.tag());
                if let Ok(next_sibling) = el.next_sibling()
                    && selector.matches(&next_sibling)
                {
                    return Self::verify(selectors, cursor + 1, &next_sibling);
                }

                false
            }
        }
    }
}

#[test]
fn test_parse_complex_relative_selector() {
    use crate::{
        css::{
            helpers::test_ok,
            patterns::Combinator,
            selectors::{compound::CompoundSelector, relative::RelativeSelectorError},
        },
        html::HtmlTag,
    };

    test_ok("", None::<ComplexRelativeSelector>);
    test_ok(
        "p",
        Some(ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Descendant,
                selector: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
            }],
        }),
    );
    test_ok(
        "p + span",
        Some(ComplexRelativeSelector {
            selectors: vec![
                RelativeSelector {
                    combinator: Combinator::Descendant,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::p),
                        ..Default::default()
                    },
                },
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::span),
                        ..Default::default()
                    },
                },
            ],
        }),
    );
    test_ok(
        "+ div",
        Some(ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::NextSibling,
                selector: CompoundSelector {
                    element: Some(HtmlTag::div),
                    ..Default::default()
                },
            }],
        }),
    );
    test_ok(
        "+ h1 + h2 +h3",
        Some(ComplexRelativeSelector {
            selectors: vec![
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::h1),
                        ..Default::default()
                    },
                },
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::h2),
                        ..Default::default()
                    },
                },
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::h3),
                        ..Default::default()
                    },
                },
            ],
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<ComplexRelativeSelector>(string, expected);
    }

    test_err(
        "p +",
        ParseError::Context(
            ComplexRelativeSelectorError::ParseFail(0).into(),
            ParseError::from(RelativeSelectorError::MissingSelector(2)).into(),
        ),
    );
}
