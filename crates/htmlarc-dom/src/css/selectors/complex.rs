use std::{fmt::Display, ops::Deref};

use thiserror::Error;

use crate::{
    css::{
        AttributeName, AttributeOperator, AttributePattern, AttributeSelector, AttributeValue,
        ClassSelector, Combinator, Context, IdSelector, IndexedError, ParseError, ParseResult,
        QuotedString, chars::CssChars, logging::debug, patterns::CssPattern,
    },
    dom::{DomRead, DomRefCell},
    html::{HtmlAttr, HtmlDoc, HtmlElement, HtmlTag, IGNORE_TAGS},
    iters::RelativeIter,
};

use super::{Selector, compound::CompoundSelector, relative::RelativeSelector};
use crate::stores::SymbolTableView;

impl ComplexSelector<'_> {
    pub(crate) fn resolve(&mut self, symbols: SymbolTableView<'_>) {
        self.first.resolve(symbols);
        for relative in &mut self.selectors {
            relative.resolve(symbols);
        }
    }
}

#[derive(Debug, Error)]
pub enum ComplexSelectorError {
    #[error("Failed to parse complex selector at {0}")]
    ParseFail(usize),
}

impl From<ComplexSelectorError> for ParseError {
    fn from(val: ComplexSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl ComplexSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for ComplexSelectorError {
    fn index(&self) -> usize {
        match *self {
            ComplexSelectorError::ParseFail(index) => index,
        }
    }
}

/// [mdn: Complex selectors](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_selectors/Selector_structure#complex_selector)
#[derive(Debug, Default, Clone)]
pub struct ComplexSelector<'s> {
    pub first: CompoundSelector<'s>,
    pub selectors: Vec<RelativeSelector<'s>>,
}

impl Display for ComplexSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.first)?;

        if self.selectors.is_empty() {
            return Ok(());
        }

        for selector in &self.selectors {
            if selector.combinator != Combinator::Descendant {
                write!(f, " {}", selector.combinator)?;
            } else {
                write!(f, " ")?;
            }
            write!(f, "{}", selector.selector)?;
        }

        Ok(())
    }
}

impl<'s> CssPattern<'s> for ComplexSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for ComplexSelector<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> ComplexSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        chars.skip_spaces();

        let Some((index, _)) = chars.current() else {
            debug!("No complex selector found at {}", chars.last_index());
            return Ok(None);
        };

        debug!("Parsing first compound selector at {}", index);
        let Some(first) =
            CompoundSelector::from_chars(chars).context(ComplexSelectorError::ParseFail(index))?
        else {
            debug!("Empty complex selector found at {}", index);
            return Ok(None);
        };

        let mut selectors = Vec::new();

        debug!("Parsing relative selectors at {}", chars.last_index());
        while let Some(selector) =
            RelativeSelector::from_chars(chars).context(ComplexSelectorError::ParseFail(index))?
        {
            selectors.push(selector);
        }

        debug!("Parsed complex selector at {}", chars.last_index());
        Ok(Some(Self { first, selectors }))
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        let mut selectors = self.selectors.iter().rev();

        Self::verify(&mut selectors, &self.first, el, &None, false)
    }

    fn verify<'a, I>(
        selectors: &mut I,
        first: &'a CompoundSelector<'a>,
        el: &HtmlElement<impl DomRead>,
        prev_connector: &Option<Combinator>,
        stop: bool,
    ) -> bool
    where
        I: Iterator<Item = &'a RelativeSelector<'a>>,
    {
        if stop {
            true
        } else {
            let (current_connector, selector, stop) = if let Some(selector) = selectors.next() {
                (selector.combinator, &selector.selector, false)
            } else {
                (Combinator::default(), first, true)
            };

            debug!("Checking selector: {}", selector);
            if let Some(combinator) = prev_connector {
                match combinator {
                    Combinator::Descendant => {
                        debug!("Checking ancestors of {}", el.tag());
                        let ancestors =
                            RelativeIter::ancestors(el).filter(|el| selector.matches(el));

                        for ancestor in ancestors {
                            debug!("Checking ancestor: {}", ancestor.tag());
                            if Self::verify(
                                selectors,
                                first,
                                &ancestor,
                                &Some(current_connector),
                                stop,
                            ) {
                                debug!("Found match: {} ", ancestor.tag(),);
                                return true;
                            }
                        }

                        false
                    }
                    Combinator::Child => {
                        debug!("Checking parent of {}", el.tag());
                        if let Ok(parent) = el.parent()
                            && selector.matches(&parent)
                        {
                            return Self::verify(
                                selectors,
                                first,
                                &parent,
                                &Some(current_connector),
                                stop,
                            );
                        }

                        false
                    }
                    Combinator::SubsequentSibling => {
                        debug!("Checking precedent siblings of {}", el.tag());
                        let siblings =
                            RelativeIter::prev_siblings(el).filter(|el| selector.matches(el));

                        for sibling in siblings {
                            debug!("Checking sibling: {}", sibling.tag());
                            if Self::verify(
                                selectors,
                                first,
                                &sibling,
                                &Some(current_connector),
                                stop,
                            ) {
                                return true;
                            }
                        }

                        false
                    }
                    Combinator::NextSibling => {
                        debug!("Checking previous sibling of {}", el.tag());
                        if let Ok(prev) = el.prev_sibling()
                            && selector.matches(&prev)
                        {
                            return Self::verify(
                                selectors,
                                first,
                                &prev,
                                &Some(current_connector),
                                stop,
                            );
                        }

                        false
                    }
                }
            } else if selector.matches(el) {
                Self::verify(selectors, first, el, &Some(current_connector), stop)
            } else {
                false
            }
        }
    }
}

#[test]
fn test_parse_complex_selector() {
    use crate::{
        css::{
            helpers::test_ok,
            patterns::Combinator,
            selectors::{
                class::ClassSelector, compound::CompoundSelectorError, id::IdSelector,
                relative::RelativeSelectorError,
            },
        },
        html::HtmlTag,
    };

    test_ok("", None::<ComplexSelector>);
    test_ok(
        "div    ",
        Some(ComplexSelector {
            first: CompoundSelector {
                element: Some(HtmlTag::div),
                ..Default::default()
            },
            selectors: Vec::new(),
        }),
    );
    test_ok(
        "a#selected > .icon",
        Some(ComplexSelector {
            first: CompoundSelector {
                element: Some(HtmlTag::a),
                id: Some(IdSelector("selected")),
                ..Default::default()
            },
            selectors: vec![RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    classes: vec![ClassSelector::new("icon")],
                    ..Default::default()
                },
            }],
        }),
    );
    test_ok(
        ".box h2 + p",
        Some(ComplexSelector {
            first: CompoundSelector {
                classes: vec![ClassSelector::new("box")],
                ..Default::default()
            },
            selectors: vec![
                RelativeSelector {
                    combinator: Combinator::Descendant,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::h2),
                        ..Default::default()
                    },
                },
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::p),
                        ..Default::default()
                    },
                },
            ],
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<ComplexSelector>(string, expected);
    }

    test_err(
        "[src]img",
        ParseError::Context(
            ComplexSelectorError::ParseFail(0).into(),
            ParseError::from(CompoundSelectorError::UnexpectedChar(5)).into(),
        ),
    );
    test_err(
        "div [src]img",
        ParseError::Context(
            ComplexSelectorError::ParseFail(0).into(),
            ParseError::Context(
                RelativeSelectorError::SelectorFail(4).into(),
                ParseError::from(CompoundSelectorError::UnexpectedChar(9)).into(),
            )
            .into(),
        ),
    );
}

#[test]
fn test_complex_match_ok() {
    fn test_match_ok(selector: ComplexSelector) {
        // <section>
        //   <div>
        //     <h1>Title</h1>
        //     <header>Header</header>
        //     <p id="content" class="red blue yellow" data-foo="bar" title="text">Paragraph</p>
        //     <span>Span</span>
        //   </div>
        // </section>
        let html = r#"<section><div><h1>Title</h1><header>Header</header><p id="content" class="red blue yellow" data-foo="bar" title="text">Paragraph</p><span>Span</span></div></section>
    "#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // section
        let el = el.first_child().unwrap(); // div
        let el = el.first_child().unwrap(); // h1
        let el = el.next_sibling().unwrap(); // header
        let el = el.next_sibling().unwrap(); // p

        assert!(selector.matches(&el));
    }

    // div > p
    test_match_ok(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::div),
            ..Default::default()
        },
        selectors: vec![RelativeSelector {
            combinator: Combinator::Child,
            selector: CompoundSelector {
                element: Some(HtmlTag::p),
                ..Default::default()
            },
        }],
    });

    // header + p
    test_match_ok(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::header),
            ..Default::default()
        },
        selectors: vec![RelativeSelector {
            combinator: Combinator::NextSibling,
            selector: CompoundSelector {
                element: Some(HtmlTag::p),
                ..Default::default()
            },
        }],
    });

    // div > h1 ~ p
    test_match_ok(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::div),
            ..Default::default()
        },
        selectors: vec![
            RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    element: Some(HtmlTag::h1),
                    ..Default::default()
                },
            },
            RelativeSelector {
                combinator: Combinator::SubsequentSibling,
                selector: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
            },
        ],
    });

    // section p
    test_match_ok(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::section),
            ..Default::default()
        },
        selectors: vec![RelativeSelector {
            combinator: Combinator::Descendant,
            selector: CompoundSelector {
                element: Some(HtmlTag::p),
                ..Default::default()
            },
        }],
    });
}

#[test]
fn test_complex_match_err() {
    fn test_match_err(selector: ComplexSelector) {
        // <section>
        //   <div>
        //     <h1>Title</h1>
        //     <header>Header</header>
        //     <p id="content" class="red blue yellow" data-foo="bar" title="text">Paragraph</p>
        //     <span>Span</span>
        //   </div>
        // </section>
        let html = r#"<section><div><h1>Title</h1><header>Header</header><p id="content" class="red blue yellow" data-foo="bar" title="text">Paragraph</p><span>Span</span></div></section>
    "#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // section
        let el = el.first_child().unwrap(); // div
        let el = el.first_child().unwrap(); // h1
        let el = el.next_sibling().unwrap(); // header
        let el = el.next_sibling().unwrap(); // p

        assert!(!selector.matches(&el));
    }

    // div > h1
    test_match_err(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::div),
            ..Default::default()
        },
        selectors: vec![RelativeSelector {
            combinator: Combinator::Child,
            selector: CompoundSelector {
                element: Some(HtmlTag::h1),
                ..Default::default()
            },
        }],
    });

    // header + span
    test_match_err(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::header),
            ..Default::default()
        },
        selectors: vec![RelativeSelector {
            combinator: Combinator::NextSibling,
            selector: CompoundSelector {
                element: Some(HtmlTag::span),
                ..Default::default()
            },
        }],
    });

    // section header ~ div > h1 ~ p
    test_match_err(ComplexSelector {
        first: CompoundSelector {
            element: Some(HtmlTag::section),
            ..Default::default()
        },
        selectors: vec![
            RelativeSelector {
                combinator: Combinator::SubsequentSibling,
                selector: CompoundSelector {
                    element: Some(HtmlTag::header),
                    ..Default::default()
                },
            },
            RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    element: Some(HtmlTag::div),
                    ..Default::default()
                },
            },
            RelativeSelector {
                combinator: Combinator::Descendant,
                selector: CompoundSelector {
                    element: Some(HtmlTag::h1),
                    ..Default::default()
                },
            },
            RelativeSelector {
                combinator: Combinator::SubsequentSibling,
                selector: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
            },
        ],
    });
}
