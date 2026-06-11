use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Combinator, ComplexRelativeSelector, ComplexSelector, CompoundSelector, Context,
        IndexedError, OrdinalPattern, ParseError, ParseResult, RelativeSelector,
        chars::CssChars,
        logging::{self, debug},
        patterns::{CssChar, CssPattern, Parentheses, Parenthesized, TextPattern},
    },
    dom::DomRead,
    html::{HtmlDoc, HtmlElement, HtmlTag},
};

use super::{Selector, list::SelectorList, list_relative::RelativeSelectorList};
use crate::stores::SymbolTableView;

impl PseudoClassSelector<'_> {
    pub(crate) fn resolve(&mut self, symbols: SymbolTableView<'_>) {
        match self {
            PseudoClassSelector::Not(list) | PseudoClassSelector::Is(list) => list.resolve(symbols),
            PseudoClassSelector::Has(relative) => relative.resolve(symbols),
            _ => {}
        }
    }
}

#[derive(Debug, Error)]
pub enum PseudoClassSelectorError {
    #[error("Failed to parse pseudo-class selector at {0}")]
    ParseFail(usize),
    #[error("Missing pseudo-class keyword at {0}")]
    MissingKeyword(usize),
    #[error("Invalid pseudo-class keyword at {0}")]
    InvalidKeyword(usize),
    #[error("Failed to parse parameter at {0}")]
    ParseParameter(usize),
    #[error("Missing parameter at {0}")]
    MissingParameter(usize),
    #[error("Empty parameter at {0}")]
    EmptyParameter(usize),
}

impl From<PseudoClassSelectorError> for ParseError {
    fn from(val: PseudoClassSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl PseudoClassSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for PseudoClassSelectorError {
    fn index(&self) -> usize {
        match *self {
            PseudoClassSelectorError::ParseFail(index) => index,
            PseudoClassSelectorError::MissingKeyword(index) => index,
            PseudoClassSelectorError::InvalidKeyword(index) => index,
            PseudoClassSelectorError::ParseParameter(index) => index,
            PseudoClassSelectorError::MissingParameter(index) => index,
            PseudoClassSelectorError::EmptyParameter(index) => index,
        }
    }
}

/// [mdn: Pseudo-classes](https://developer.mozilla.org/en-US/docs/Web/CSS/Pseudo-classes)
#[derive(Debug, Clone)]
pub enum PseudoClassSelector<'s> {
    Root,
    Empty,
    NthChild(OrdinalPattern),
    NthLastChild(OrdinalPattern),
    FirstChild,
    LastChild,
    OnlyChild,
    NthOfType(OrdinalPattern),
    NthLastOfType(OrdinalPattern),
    FirstOfType,
    LastOfType,
    OnlyOfType,
    Not(SelectorList<'s>),
    Has(RelativeSelectorList<'s>),
    Is(SelectorList<'s>),
}

impl Display for PseudoClassSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PseudoClassSelector::Root => write!(f, ":root"),
            PseudoClassSelector::Empty => write!(f, ":empty"),
            PseudoClassSelector::NthChild(formula) => {
                write!(f, ":nth-child({formula})")
            }
            PseudoClassSelector::NthLastChild(formula) => {
                write!(f, ":nth-last-child({formula})")
            }
            PseudoClassSelector::FirstChild => write!(f, ":first-child"),
            PseudoClassSelector::LastChild => write!(f, ":last-child"),
            PseudoClassSelector::OnlyChild => write!(f, ":only-child"),
            PseudoClassSelector::NthOfType(formula) => {
                write!(f, ":nth-of-type({formula})")
            }
            PseudoClassSelector::NthLastOfType(formula) => {
                write!(f, ":nth-last-of-type({formula})")
            }
            PseudoClassSelector::FirstOfType => write!(f, ":first-of-type"),
            PseudoClassSelector::LastOfType => write!(f, ":last-of-type"),
            PseudoClassSelector::OnlyOfType => write!(f, ":only-of-type"),
            PseudoClassSelector::Not(selector_list) => write!(f, ":not({})", selector_list),
            PseudoClassSelector::Has(selector_list) => write!(f, ":has({})", selector_list),
            PseudoClassSelector::Is(selector_list) => write!(f, ":is({})", selector_list),
        }
    }
}

impl<'s> CssPattern<'s> for PseudoClassSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for PseudoClassSelector<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> PseudoClassSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((colon_index, colon_char)) = chars.current() else {
            debug!("No pseudo-class selector found at {}", chars.last_index());
            return Ok(None);
        };

        if colon_char != ':' {
            debug!("Not a pseudo-class selector at {}", colon_index);
            return Ok(None);
        }

        let pattern = TextPattern::default()
            .allow_alphabetic()
            .start_with(CssChar::Alphabetic)
            .allow_special('-')
            .stop_at('(')
            .stop_at(':')
            .stop_at('.')
            .stop_at('#')
            .stop_at(' ')
            .stop_at('>')
            .stop_at('~')
            .stop_at('+')
            .stop_at('\n');

        chars.next();

        debug!("Parsing pseudo-class name at {}", chars.last_index());
        if let Some(name) = pattern
            .validate(chars)
            .context(PseudoClassSelectorError::ParseFail(chars.last_index()))?
        {
            let parameter_index = chars.last_index();
            match name {
                "root" => {
                    debug!("Parsed root selector at {}", chars.last_index());
                    Ok(Some(Self::Root))
                }
                "empty" => {
                    debug!("Parsed empty selector at {}", chars.last_index());
                    Ok(Some(Self::Empty))
                }
                "nth-child" => {
                    debug!("Parsing nth-child parameter at {}", parameter_index);
                    let content: Option<Parenthesized<Parentheses, OrdinalPattern>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(ordinal) = parenthesized.inner() {
                            debug!("Parsed nth-child selector at {}", chars.last_index());
                            Ok(Some(Self::NthChild(ordinal)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "nth-last-child" => {
                    debug!("Parsing nth-last-child parameter at {}", parameter_index);
                    let content: Option<Parenthesized<Parentheses, OrdinalPattern>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(ordinal) = parenthesized.inner() {
                            debug!("Parsed nth-last-child selector at {}", chars.last_index());
                            Ok(Some(Self::NthLastChild(ordinal)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "first-child" => {
                    debug!("Parsed first-child selector at {}", chars.last_index());
                    Ok(Some(Self::FirstChild))
                }
                "last-child" => {
                    debug!("Parsed last-child selector at {}", chars.last_index());
                    Ok(Some(Self::LastChild))
                }
                "only-child" => {
                    debug!("Parsed only-child selector at {}", chars.last_index());
                    Ok(Some(Self::OnlyChild))
                }
                "nth-of-type" => {
                    debug!("Parsing nth-of-type parameter at {}", parameter_index);
                    let content: Option<Parenthesized<Parentheses, OrdinalPattern>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(ordinal) = parenthesized.inner() {
                            debug!("Parsed nth-of-type selector at {}", chars.last_index());
                            Ok(Some(Self::NthOfType(ordinal)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "nth-last-of-type" => {
                    debug!("Parsing nth-last-of-type parameter at {}", parameter_index);
                    let content: Option<Parenthesized<Parentheses, OrdinalPattern>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(ordinal) = parenthesized.inner() {
                            debug!("Parsed nth-last-of-type selector at {}", chars.last_index());
                            Ok(Some(Self::NthLastOfType(ordinal)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "first-of-type" => {
                    debug!("Parsed first-of-type selector at {}", chars.last_index());
                    Ok(Some(Self::FirstOfType))
                }
                "last-of-type" => {
                    debug!("Parsed last-of-type selector at {}", chars.last_index());
                    Ok(Some(Self::LastOfType))
                }
                "only-of-type" => {
                    debug!("Parsed only-of-type selector at {}", chars.last_index());
                    Ok(Some(Self::OnlyOfType))
                }
                "not" => {
                    debug!("Parsing not selector parameter at {}", chars.last_index());
                    let content: Option<Parenthesized<Parentheses, SelectorList>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(selectors) = parenthesized.inner() {
                            debug!("Parsed not selector at {}", chars.last_index());
                            Ok(Some(Self::Not(selectors)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "has" => {
                    debug!("Parsing has selector parameter at {}", chars.last_index());
                    let content: Option<Parenthesized<Parentheses, RelativeSelectorList>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(selectors) = parenthesized.inner() {
                            debug!("Parsed has selector at {}", chars.last_index());
                            Ok(Some(Self::Has(selectors)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                "is" => {
                    debug!("Parsing is selector parameter at {}", chars.last_index());
                    let content: Option<Parenthesized<Parentheses, SelectorList>> =
                        Parenthesized::from_chars(chars)
                            .context(PseudoClassSelectorError::ParseParameter(parameter_index))?;

                    if let Some(parenthesized) = content {
                        if let Some(selectors) = parenthesized.inner() {
                            debug!("Parsed is selector at {}", chars.last_index());
                            Ok(Some(Self::Is(selectors)))
                        } else {
                            Err(PseudoClassSelectorError::EmptyParameter(chars.last_index()).into())
                        }
                    } else {
                        Err(PseudoClassSelectorError::MissingParameter(chars.last_index()).into())
                    }
                }
                _ => Err(PseudoClassSelectorError::InvalidKeyword(colon_index + 1).into()),
            }
        } else {
            Err(PseudoClassSelectorError::MissingKeyword(colon_index).into())
        }
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        match self {
            PseudoClassSelector::Root => el.is_root(),
            PseudoClassSelector::Empty => el.has_no_children(),
            PseudoClassSelector::NthChild(ordinal_pattern) => {
                let position = el.nth_position(|_| true);
                ordinal_pattern.matches(position)
            }
            PseudoClassSelector::NthLastChild(ordinal_pattern) => {
                let position = el.nth_reverse_position(|_| true);
                ordinal_pattern.matches(position)
            }
            PseudoClassSelector::FirstChild => el.is_first_child(),
            PseudoClassSelector::LastChild => el.is_last_child(),
            PseudoClassSelector::OnlyChild => el.is_first_child() && el.is_last_child(),
            PseudoClassSelector::NthOfType(ordinal_pattern) => {
                let position = el.nth_position(|element| element.tag() == el.tag());
                ordinal_pattern.matches(position)
            }
            PseudoClassSelector::NthLastOfType(ordinal_pattern) => {
                let position = el.nth_reverse_position(|element| element.tag() == el.tag());
                ordinal_pattern.matches(position)
            }
            PseudoClassSelector::FirstOfType => el.is_first_of_type(),
            PseudoClassSelector::LastOfType => el.is_last_of_type(),
            PseudoClassSelector::OnlyOfType => el.is_only_of_type(),
            PseudoClassSelector::Not(selector_list) => !selector_list.matches(el),
            PseudoClassSelector::Has(relative_selector_list) => relative_selector_list.matches(el),
            PseudoClassSelector::Is(selector_list) => selector_list.matches(el),
        }
    }
}

#[test]
fn test_parse_pseudo_class_selector() {
    use crate::{
        css::{
            helpers::test_ok,
            patterns::{Combinator, OrdinalPattern, ParenthesizedError, TextPatternError},
            selectors::{
                complex::ComplexSelector, complex_relative::ComplexRelativeSelector,
                compound::CompoundSelector, relative::RelativeSelector,
            },
        },
        html::HtmlTag,
    };
    test_ok("", None::<PseudoClassSelector>);
    test_ok(":root", Some(PseudoClassSelector::Root));
    test_ok(":empty", Some(PseudoClassSelector::Empty));
    test_ok(
        ":nth-child(2n+1)",
        Some(PseudoClassSelector::NthChild(OrdinalPattern::Formula {
            backward: false,
            a: 2,
            b: Some(1),
        })),
    );
    test_ok(
        ":nth-last-child(n)",
        Some(PseudoClassSelector::NthLastChild(OrdinalPattern::Formula {
            backward: false,
            a: 1,
            b: None,
        })),
    );
    test_ok(":first-child", Some(PseudoClassSelector::FirstChild));
    test_ok(":last-child", Some(PseudoClassSelector::LastChild));
    test_ok(":only-child", Some(PseudoClassSelector::OnlyChild));
    test_ok(
        ":nth-of-type(3n)",
        Some(PseudoClassSelector::NthOfType(OrdinalPattern::Formula {
            backward: false,
            a: 3,
            b: None,
        })),
    );
    test_ok(
        ":nth-last-of-type(2n+1)",
        Some(PseudoClassSelector::NthLastOfType(
            OrdinalPattern::Formula {
                backward: false,
                a: 2,
                b: Some(1),
            },
        )),
    );
    test_ok(":first-of-type", Some(PseudoClassSelector::FirstOfType));
    test_ok(":last-of-type", Some(PseudoClassSelector::LastOfType));
    test_ok(":only-of-type", Some(PseudoClassSelector::OnlyOfType));
    test_ok(
        ":not(div,a, h1)",
        Some(PseudoClassSelector::Not(SelectorList {
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
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::h1),
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
            ],
        })),
    );
    test_ok(
        ":has(+h1,+h2, +h3)",
        Some(PseudoClassSelector::Has(RelativeSelectorList {
            selectors: vec![
                ComplexRelativeSelector {
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::NextSibling,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::h1),
                            ..Default::default()
                        },
                    }],
                },
                ComplexRelativeSelector {
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::NextSibling,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::h2),
                            ..Default::default()
                        },
                    }],
                },
                ComplexRelativeSelector {
                    selectors: vec![RelativeSelector {
                        combinator: Combinator::NextSibling,
                        selector: CompoundSelector {
                            element: Some(HtmlTag::h3),
                            ..Default::default()
                        },
                    }],
                },
            ],
        })),
    );
    test_ok(
        ":is(div:first-child,span, i)",
        Some(PseudoClassSelector::Is(SelectorList {
            selectors: vec![
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::div),
                        pseudo_classes: vec![PseudoClassSelector::FirstChild],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::span),
                        id: None,
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
                ComplexSelector {
                    first: CompoundSelector {
                        element: Some(HtmlTag::i),
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                },
            ],
        })),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<PseudoClassSelector>(string, expected);
    }

    test_err(":", PseudoClassSelectorError::MissingKeyword(0).into());
    test_err(
        ":-",
        ParseError::Context(
            PseudoClassSelectorError::ParseFail(1).into(),
            ParseError::from(TextPatternError::StartsWith(1, '-')).into(),
        ),
    );
    test_err(":test", PseudoClassSelectorError::InvalidKeyword(1).into());
    test_err(
        ":nth-child",
        PseudoClassSelectorError::MissingParameter(9).into(),
    );
    test_err(
        ":nth-child()",
        PseudoClassSelectorError::EmptyParameter(11).into(),
    );
    test_err(
        ":nth-of-type(",
        ParseError::Context(
            PseudoClassSelectorError::ParseParameter(12).into(),
            ParseError::from(ParenthesizedError::MissingContent(12)).into(),
        ),
    );
}

#[test]
fn test_pseudo_class_matching_ok() {
    let html = "<div></div>";
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();

    let selector = PseudoClassSelector::Root;
    assert!(selector.matches(&el));

    let el = el.first_child().unwrap(); // div
    let selector = PseudoClassSelector::Empty;
    assert!(selector.matches(&el));

    let selector = PseudoClassSelector::FirstChild;
    assert!(selector.matches(&el));

    let selector = PseudoClassSelector::LastChild;
    assert!(selector.matches(&el));

    let selector = PseudoClassSelector::FirstOfType;
    assert!(selector.matches(&el));

    let selector = PseudoClassSelector::LastOfType;
    assert!(selector.matches(&el));

    let selector = PseudoClassSelector::OnlyOfType;
    assert!(selector.matches(&el));

    // :not(p, a > span)
    let selector = PseudoClassSelector::Not(SelectorList {
        selectors: vec![
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::a),
                    ..Default::default()
                },
                selectors: vec![RelativeSelector {
                    combinator: Combinator::Child,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::span),
                        ..Default::default()
                    },
                }],
            },
        ],
    });
    assert!(selector.matches(&el));

    // :is(p > span, div)
    let selector = PseudoClassSelector::Is(SelectorList {
        selectors: vec![
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
                selectors: vec![RelativeSelector {
                    combinator: Combinator::Child,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::span),
                        ..Default::default()
                    },
                }],
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::div),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
        ],
    });
    assert!(selector.matches(&el));

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

    // :has(>header)
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    element: Some(HtmlTag::header),
                    ..Default::default()
                },
            }],
        }],
    });
    assert!(selector.matches(&el));

    // :has(aside)
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Descendant,
                selector: CompoundSelector {
                    element: Some(HtmlTag::aside),
                    ..Default::default()
                },
            }],
        }],
    });
    let el = doc.root();
    let el = el.first_child().unwrap(); // section
    assert!(selector.matches(&el));

    // :has(+div > p)
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![
                RelativeSelector {
                    combinator: Combinator::NextSibling,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::div),
                        ..Default::default()
                    },
                },
                RelativeSelector {
                    combinator: Combinator::Child,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::p),
                        ..Default::default()
                    },
                },
            ],
        }],
    });
    let el = doc.root();
    let el = el.first_child().unwrap(); // section
    let el = el.first_child().unwrap(); // header
    assert!(selector.matches(&el));

    // :has(>:not(p))
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    pseudo_classes: vec![PseudoClassSelector::Not(SelectorList {
                        selectors: vec![ComplexSelector {
                            first: CompoundSelector {
                                element: Some(HtmlTag::p),
                                id: None,
                                ..Default::default()
                            },
                            selectors: Vec::new(),
                        }],
                    })],
                    ..Default::default()
                },
            }],
        }],
    });
    let el = doc.root();
    let el = el.first_child().unwrap(); // section
    assert!(selector.matches(&el));

    // <div>
    //  <h1>1</h1>
    //  <p>2</p>
    //  <span>3</span>
    // </div>
    let html = r#"<div><h1></h1><p></p><span></span></div>"#;
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();
    let el = el.first_child().unwrap(); // div
    let el = el.last_child_all().unwrap(); // span

    // :nth-child(3)
    let selector = PseudoClassSelector::NthChild(OrdinalPattern::N(3));
    println!("{}", selector);
    assert!(selector.matches(&el));

    // <dl>
    //   <dt>Vegetables:</dt>
    //   <dd>1. Tomatoes</dd>
    //   <dd>2. Cucumbers</dd>
    //   <dd>3. Mushrooms</dd>
    //   <dt>Fruits:</dt>
    //   <dd>4. Apples</dd>
    //   <dd>5. Mangos</dd>
    //   <dd>6. Pears</dd>
    //   <dd>7. Oranges</dd>
    // </dl>
    let html = r#"<dl><dt>Vegetables:</dt><dd>1. Tomatoes</dd><dd>2. Cucumbers</dd><dd>3. Mushrooms</dd><dt>Fruits:</dt><dd>4. Apples</dd><dd>5. Mangos</dd><dd>6. Pears</dd><dd>7. Oranges</dd></dl>"#;
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();
    let el = el.first_child().unwrap(); // dl
    let el = el.last_child_all().unwrap(); // dd 7
    let el = el.prev_sibling().unwrap(); // dd 6
    let el = el.prev_sibling().unwrap(); // dd 5

    // :nth-of-type(5)
    let selector = PseudoClassSelector::NthOfType(OrdinalPattern::N(5));
    assert!(selector.matches(&el));
}

#[test]
fn test_pseudo_class_matching_err() {
    let html = "<div><p>1</p><p>2</p><p>3</p></div>";
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();
    let el = el.first_child().unwrap(); // div

    let selector = PseudoClassSelector::Root;
    assert!(!selector.matches(&el));

    let selector = PseudoClassSelector::Empty;
    assert!(!selector.matches(&el));

    let el = el.first_child().unwrap(); // p 1
    let el = el.next_sibling().unwrap(); // p 2

    let selector = PseudoClassSelector::FirstChild;
    assert!(!selector.matches(&el));

    let selector = PseudoClassSelector::LastChild;
    assert!(!selector.matches(&el));

    let selector = PseudoClassSelector::FirstOfType;
    assert!(!selector.matches(&el));

    let selector = PseudoClassSelector::LastOfType;
    assert!(!selector.matches(&el));

    let selector = PseudoClassSelector::OnlyOfType;
    assert!(!selector.matches(&el));

    // :not(a > span, p)
    let selector = PseudoClassSelector::Not(SelectorList {
        selectors: vec![
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::a),
                    ..Default::default()
                },
                selectors: vec![RelativeSelector {
                    combinator: Combinator::Child,
                    selector: CompoundSelector {
                        element: Some(HtmlTag::span),
                        ..Default::default()
                    },
                }],
            },
            ComplexSelector {
                first: CompoundSelector {
                    element: Some(HtmlTag::p),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
        ],
    });
    assert!(!selector.matches(&el));

    // is:(div, span)
    let selector = PseudoClassSelector::Is(SelectorList {
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
                    element: Some(HtmlTag::span),
                    ..Default::default()
                },
                selectors: Vec::new(),
            },
        ],
    });
    assert!(!selector.matches(&el));

    // :has(+header)
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::NextSibling,
                selector: CompoundSelector {
                    element: Some(HtmlTag::header),
                    ..Default::default()
                },
            }],
        }],
    });
    assert!(!selector.matches(&el));

    // :has(span)
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Descendant,
                selector: CompoundSelector {
                    element: Some(HtmlTag::span),
                    ..Default::default()
                },
            }],
        }],
    });
    let el = doc.root();
    let el = el.first_child().unwrap(); // div
    assert!(!selector.matches(&el));

    // :has(>:not(p))
    let selector = PseudoClassSelector::Has(RelativeSelectorList {
        selectors: vec![ComplexRelativeSelector {
            selectors: vec![RelativeSelector {
                combinator: Combinator::Child,
                selector: CompoundSelector {
                    pseudo_classes: vec![PseudoClassSelector::Not(SelectorList {
                        selectors: vec![ComplexSelector {
                            first: CompoundSelector {
                                element: Some(HtmlTag::p),
                                ..Default::default()
                            },
                            selectors: Vec::new(),
                        }],
                    })],
                    ..Default::default()
                },
            }],
        }],
    });
    let el = doc.root();
    let el = el.first_child().unwrap(); // div
    assert!(!selector.matches(&el));

    // <div>
    //  <h1>1</h1>
    //  <p>2</p>
    //  <span>3</span>
    // </div>
    let html = r#"<div><h1></h1><p></p><span></span></div>"#;
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();
    let el = el.first_child().unwrap(); // div
    let el = el.last_child_all().unwrap(); // span

    // :nth-child(4)
    let selector = PseudoClassSelector::NthChild(OrdinalPattern::N(4));
    assert!(!selector.matches(&el));

    // <dl>
    //   <dt>Vegetables:</dt>
    //   <dd>1. Tomatoes</dd>
    //   <dd>2. Cucumbers</dd>
    //   <dd>3. Mushrooms</dd>
    //   <dt>Fruits:</dt>
    //   <dd>4. Apples</dd>
    //   <dd>5. Mangos</dd>
    //   <dd>6. Pears</dd>
    //   <dd>7. Oranges</dd>
    // </dl>
    let html = r#"<dl><dt>Vegetables:</dt><dd>1. Tomatoes</dd><dd>2. Cucumbers</dd><dd>3. Mushrooms</dd><dt>Fruits:</dt><dd>4. Apples</dd><dd>5. Mangos</dd><dd>6. Pears</dd><dd>7. Oranges</dd></dl>"#;
    let doc = HtmlDoc::parse(html).unwrap().dom();
    let el = doc.root();
    let el = el.first_child().unwrap(); // dl
    let el = el.last_child_all().unwrap(); // dd 7
    let el = el.prev_sibling().unwrap(); // dd 6
    let el = el.prev_sibling().unwrap(); // dd 5

    // :nth-of-type(6)
    let selector = PseudoClassSelector::NthOfType(OrdinalPattern::N(6));
    assert!(!selector.matches(&el));
}
