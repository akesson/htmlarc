use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeName, AttributePattern, AttributeSelector, ClassSelector, Combinator,
        CompoundSelector, Context, IndexedError, ParseError, ParseResult, RelativeSelector,
        chars::CssChars, logging::debug, patterns::CssPattern,
    },
    dom::DomRead,
    html::{HtmlAttr, HtmlDoc, HtmlElement, HtmlTag},
};

use super::{Selector, complex::ComplexSelector};
use crate::dom::DomView;

impl SelectorList<'_> {
    /// Bind every class selector in the tree to a document's symbols (the resolve pass run
    /// once by [`MatchIter`](crate::iters::MatchIter)). See [`super::ClassSelector::resolve`].
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        for selector in &mut self.selectors {
            selector.resolve(view);
        }
    }

    /// View-based counterpart of [`Selector::matches`] used by the bound-view `select` walk
    /// (ADR 0007): match against a [`DomView`] bound once for the whole walk instead of
    /// rebuilding it per accessor. `el` carries the node index and is the fallback path for the
    /// parts that cannot read the (text-empty) bound view — combinators, text, pseudo-classes.
    pub(crate) fn matches_in_view(&self, view: &DomView, el: &HtmlElement<impl DomRead>) -> bool {
        self.selectors.iter().any(|s| s.matches_in_view(view, el))
    }
}

#[derive(Debug, Error)]
pub enum SelectorListError {
    #[error("Failed to parse selector list at {0}")]
    ParseFail(usize),
    #[error("Unterminated selector list at {0}")]
    Unterminated(usize),
}

impl From<SelectorListError> for ParseError {
    fn from(val: SelectorListError) -> Self {
        val.into_parse_error()
    }
}

impl SelectorListError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for SelectorListError {
    fn index(&self) -> usize {
        match *self {
            SelectorListError::ParseFail(index) => index,
            SelectorListError::Unterminated(index) => index,
        }
    }
}

/// [mdn: Selector lists](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_selectors/Selector_structure#selector_list)
#[derive(Debug, Clone)]
pub struct SelectorList<'s> {
    pub selectors: Vec<ComplexSelector<'s>>,
}

impl Display for SelectorList<'_> {
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

impl<'s> CssPattern<'s> for SelectorList<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for SelectorList<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.selectors.iter().any(|s| s.matches(el))
    }
}

impl<'s> SelectorList<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((index, _)) = chars.current() else {
            debug!("No selector list found at {}", chars.last_index());
            return Ok(None);
        };

        let mut selectors = Vec::new();

        let mut expect_selector = true;

        while let Some(selector) =
            ComplexSelector::from_chars(chars).context(SelectorListError::ParseFail(index))?
        {
            selectors.push(selector);
            chars.skip_spaces();
            expect_selector = false;
            debug!("Adding complex selector to list at {}", chars.last_index());

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
            return Err(SelectorListError::Unterminated(chars.last_index()).into());
        }

        debug!("Parsed selector list at {}", chars.last_index());
        Ok(Some(Self { selectors }))
    }
}

#[test]
fn test_parse_selector_list() {
    use crate::{
        css::{
            helpers::test_ok,
            patterns::Combinator,
            selectors::{
                class::ClassSelector,
                complex::ComplexSelectorError,
                compound::{CompoundSelector, CompoundSelectorError},
                id::IdSelector,
                relative::RelativeSelector,
            },
        },
        html::HtmlTag,
    };

    test_ok("", None::<SelectorList>);
    test_ok(
        "div, a",
        Some(SelectorList {
            selectors: vec![
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::div),
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::a),
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
            ],
        }),
    );
    test_ok(
        "div#header > p   ,  span.blue",
        Some(SelectorList {
            selectors: vec![
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::div),
                        id: Some(IdSelector::new("header")),
                        ..Default::default()
                    },
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::Child,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::p),
                            ..Default::default()
                        },
                    }],
                },
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::span),
                        classes: vec![ClassSelector::new("blue")],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
            ],
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<SelectorList>(string, expected);
    }

    test_err("div, ", SelectorListError::Unterminated(4).into());
    test_err(
        "div, [src]img",
        ParseError::Context(
            SelectorListError::ParseFail(0).into(),
            ParseError::Context(
                ComplexSelectorError::ParseFail(5).into(),
                ParseError::from(CompoundSelectorError::UnexpectedChar(10)).into(),
            )
            .into(),
        ),
    );
}

#[test]
fn test_selector_list_matching_ok() {
    fn test_match(selector: SelectorList) {
        // <section>
        //   <header>title</header>
        //   <div id="main">
        //     <p>content</p>
        //     <aside class="red">sidebar</aside>
        //   </div>
        // </section>
        let html = r#"<section><header>title</header><div id="main"><p>content</p><aside class="red">sidebar</aside></div></section>"#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // section

        assert!(selector.matches(&el));
    }

    // section, header, div, p, aside
    test_match(SelectorList {
        selectors: vec![
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::section),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::header),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::div),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::aside),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
        ],
    });
}

#[test]
fn test_selector_list_matching_err() {
    fn test_match(selector: SelectorList) {
        // <section>
        //   <header>title</header>
        //   <div id="main">
        //     <p>content</p>
        //     <aside class="red">sidebar</aside>
        //   </div>
        // </section>
        let html = r#"<section><header>title</header><div id="main"><p>content</p><aside class="red">sidebar</aside></div></section>"#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // section

        assert!(!selector.matches(&el));
    }

    // section.blue, header > h1, div[title], p.red, aside.blue
    test_match(SelectorList {
        selectors: vec![
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::section),
                    classes: vec![ClassSelector::new("blue")],
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::header),
                    ..Default::default()
                },
                selectors: vec![RelativeSelector {
                    combinator: Combinator::Child,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::h1),
                        ..Default::default()
                    },
                }],
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::div),
                    attributes: vec![AttributeSelector::new(AttributePattern {
                        name: AttributeName::Std(HtmlAttr::title),
                        value: None,
                    })],
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::p),
                    classes: vec![ClassSelector::new("red")],
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::aside),
                    classes: vec![ClassSelector::new("blue")],
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
        ],
    });
}
