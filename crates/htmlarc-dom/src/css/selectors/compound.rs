use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeName, AttributeOperator, AttributePattern, AttributeValue, CaseIndicator, Context,
        IndexedError, ParseError, ParseResult, QuotedString,
        chars::CssChars,
        logging::debug,
        patterns::CssPattern,
        selectors::{pseudo_class::PseudoClassSelector, tag::TagSelector},
    },
    dom::DomRead,
    html::{HtmlAttr, HtmlDoc, HtmlElement, HtmlTag},
    iters::DomIterator,
};

use super::{Selector, attribute::AttributeSelector, class::ClassSelector, id::IdSelector};

#[derive(Debug, Error)]
pub enum CompoundSelectorError {
    #[error("Duplicate id selector at {0}")]
    DuplicateId(usize),
    #[error("Expected class selector at {0}")]
    ExpectedClass(usize),
    #[error("Expected id selector at {0}")]
    ExpectedId(usize),
    #[error("Expected attribute selector at {0}")]
    ExpectedAttribute(usize),
    #[error("Expected pseudo-class selector at {0}")]
    ExpectedPseudoClass(usize),
    #[error("Unexpected alphabetic character at {0}")]
    UnexpectedChar(usize),
    #[error("Failed to parse class selector at {0}")]
    ClassFail(usize),
    #[error("Failed to parse id selector at {0}")]
    IdFail(usize),
    #[error("Failed to parse attribute selector at {0}")]
    AttributeFail(usize),
    #[error("Failed to parse pseudo-class selector at {0}")]
    PseudoClassFail(usize),
}

impl From<CompoundSelectorError> for ParseError {
    fn from(value: CompoundSelectorError) -> Self {
        value.into_parse_error()
    }
}

impl CompoundSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for CompoundSelectorError {
    fn index(&self) -> usize {
        match *self {
            CompoundSelectorError::DuplicateId(index) => index,
            CompoundSelectorError::ExpectedClass(index) => index,
            CompoundSelectorError::ExpectedId(index) => index,
            CompoundSelectorError::ExpectedAttribute(index) => index,
            CompoundSelectorError::ExpectedPseudoClass(index) => index,
            CompoundSelectorError::UnexpectedChar(index) => index,
            CompoundSelectorError::ClassFail(index) => index,
            CompoundSelectorError::IdFail(index) => index,
            CompoundSelectorError::AttributeFail(index) => index,
            CompoundSelectorError::PseudoClassFail(index) => index,
        }
    }
}

/// [mdn: Compound selectors](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_selectors/Selector_structure#compound_selector)
#[derive(Debug, Default, Clone)]
pub struct CompoundSelector<'s> {
    pub element: Option<HtmlTag>,
    pub id: Option<IdSelector<'s>>,
    pub classes: Vec<ClassSelector<'s>>,
    pub attributes: Vec<AttributeSelector<'s>>,
    pub class_attributes: Vec<AttributeSelector<'s>>,
    pub data_attributes: Vec<AttributeSelector<'s>>,
    pub pseudo_classes: Vec<PseudoClassSelector<'s>>,
    pub text: Option<AttributePattern<'s>>,
}

impl Display for CompoundSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(tag) = &self.element {
            write!(f, "{}", tag)?;
        }
        if let Some(id) = &self.id {
            write!(f, "{}", id)?;
        }
        for attribute in &self.attributes {
            write!(f, "{}", attribute)?;
        }
        if let Some(text) = &self.text {
            write!(f, "[{}]", text)?;
        }
        for class in &self.class_attributes {
            write!(f, "{}", class)?;
        }
        for data in &self.data_attributes {
            write!(f, "{}", data)?;
        }
        for pseudo_class in &self.pseudo_classes {
            write!(f, "{}", pseudo_class)?;
        }
        for class in &self.classes {
            write!(f, "{}", class)?;
        }
        Ok(())
    }
}

impl<'s> CssPattern<'s> for CompoundSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for CompoundSelector<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> CompoundSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((_, c)) = chars.current() else {
            debug!("No compound selector found at {}", chars.last_index());
            return Ok(None);
        };

        if c == ',' || c == ')' {
            debug!("Empty compound selector found at {}", chars.last_index());
            return Ok(None);
        }

        debug!("Trying to parse tag selector at {}", chars.last_index());
        let element = TagSelector::from_chars(chars)?;

        let mut compound = Self {
            element: element.map(|tag| tag.inner()),
            id: None,
            classes: Vec::new(),
            attributes: Vec::new(),
            class_attributes: Vec::new(),
            data_attributes: Vec::new(),
            pseudo_classes: Vec::new(),
            text: None,
        };

        while let Some((index, char)) = chars.current() {
            match char {
                '.' => {
                    debug!("Parsing class selector at {}", index);
                    if let Some(class) = ClassSelector::from_chars(chars)
                        .context(CompoundSelectorError::ClassFail(index))?
                    {
                        compound.classes.push(class);
                    } else {
                        return Err(CompoundSelectorError::ExpectedClass(index).into());
                    }
                }
                '#' => {
                    debug!("Parsing id selector at {}", index);
                    if let Some(id) = IdSelector::from_chars(chars)
                        .context(CompoundSelectorError::IdFail(index))?
                    {
                        if let Some(c_id) = compound.id {
                            debug!("Duplicate id selector #{} at {}", c_id.0, index);
                            if c_id.0 != id.0 {
                                return Err(CompoundSelectorError::DuplicateId(index).into());
                            }
                        }
                        compound.id = Some(id);
                    } else {
                        return Err(CompoundSelectorError::ExpectedId(index).into());
                    }
                }
                '[' => {
                    debug!("Parsing attribute selector at {}", index);
                    if let Some(attribute) = AttributeSelector::from_chars(chars)
                        .context(CompoundSelectorError::AttributeFail(index))?
                    {
                        if let AttributeName::Data(_) = attribute.0.name {
                            compound.data_attributes.push(attribute);
                        } else if let AttributeName::Html(attr) = attribute.0.name {
                            if attr == HtmlAttr::class {
                                compound.class_attributes.push(attribute);
                            } else {
                                compound.attributes.push(attribute);
                            }
                        } else if let AttributeName::Text = attribute.0.name {
                            compound.text = Some(attribute.0);
                        }
                    } else {
                        return Err(CompoundSelectorError::ExpectedAttribute(index).into());
                    }
                }
                ':' => {
                    debug!("Parsing pseudo-class selector at {}", index);
                    if let Some(pseudo_class) = PseudoClassSelector::from_chars(chars)
                        .context(CompoundSelectorError::PseudoClassFail(index))?
                    {
                        compound.pseudo_classes.push(pseudo_class);
                    } else {
                        return Err(CompoundSelectorError::ExpectedPseudoClass(index).into());
                    }
                }
                c if c.is_alphabetic() => {
                    return Err(CompoundSelectorError::UnexpectedChar(index).into());
                }
                _ => break,
            }
        }

        debug!("Parsed compound selector at {}", chars.last_index());
        Ok(Some(compound))
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        debug!(
            "Matching compound selector {} to element {}",
            self,
            el.tag()
        );

        if let Some(tag) = self.element
            && tag != el.tag()
        {
            debug!("Tag mismatch: {} != {}", tag, el.tag());
            return false;
        }

        if let Some(id) = &self.id
            && !el.has_id(id.0)
        {
            debug!("Id mismatch: {} ", id.0);
            return false;
        }

        if let Some(text_pattern) = &self.text {
            let mut text_iter = el.descendants().text_chars();
            if let Some(value) = &text_pattern.value {
                let (search, other) = if let Some(CaseIndicator::Insensitive) = &value.case {
                    let search = value.value.0.to_lowercase();
                    let other: String = text_iter.collect();
                    (search, other)
                } else {
                    (value.value.0.to_string(), text_iter.collect())
                };
                if !value.operator.matches(&search, &other) {
                    return false;
                }
            } else if text_iter.next().is_none() {
                debug!("Has no text content");
                return false;
            }
        }

        for pseudo_class in &self.pseudo_classes {
            if !pseudo_class.matches(el) {
                return false;
            }
        }

        if !self.attributes.is_empty() && !el.has_attributes(&self.attributes) {
            debug!("Attributes mismatch");
            return false;
        }
        if !self.data_attributes.is_empty() && !el.has_data_attributes(&self.data_attributes) {
            debug!("Data attributes mismatch");
            return false;
        }

        if !self.classes.is_empty() && !el.has_classes(&self.classes) {
            debug!("Classes mismatch");
            return false;
        }

        if !self.class_attributes.is_empty() && !el.has_classes(&self.class_attributes) {
            debug!("Class attributes mismatch");
            return false;
        }

        true
    }
}

#[test]
fn test_parse_compound_selector() {
    use crate::{
        css::{
            ParseError,
            helpers::test_ok,
            patterns::{
                AttributeOperator, AttributePattern, AttributeValue, ParenthesizedError,
                QuotedString,
            },
            selectors::{
                attribute::AttributeSelectorError,
                class::ClassSelectorError,
                id::IdSelectorError,
                pseudo_class::{PseudoClassSelector, PseudoClassSelectorError},
            },
        },
        html::{HtmlAttr, HtmlTag},
    };

    test_ok("", None::<CompoundSelector>);
    test_ok(
        "div",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            ..Default::default()
        }),
    );
    test_ok(
        "[text*=\"test\"]",
        Some(CompoundSelector {
            text: Some(AttributePattern {
                name: AttributeName::Text,
                value: Some(AttributeValue {
                    operator: AttributeOperator::Includes,
                    value: QuotedString("test".into()),
                    case: None,
                }),
            }),
            ..Default::default()
        }),
    );
    test_ok(
        "div#main",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            id: Some(IdSelector("main")),
            ..Default::default()
        }),
    );
    test_ok(
        "div#footer.black",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            id: Some(IdSelector("footer")),
            classes: vec![ClassSelector("black")],
            ..Default::default()
        }),
    );
    test_ok(
        "div.red#header.green",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            id: Some(IdSelector("header")),
            classes: vec![ClassSelector("red"), ClassSelector("green")],
            ..Default::default()
        }),
    );
    test_ok(
        "div[class]",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            class_attributes: vec![AttributeSelector(AttributePattern {
                name: AttributeName::Html(HtmlAttr::class),
                value: None,
            })],
            ..Default::default()
        }),
    );
    test_ok(
        "div.red[class][data-test]",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            classes: vec![ClassSelector("red")],
            class_attributes: vec![AttributeSelector(AttributePattern {
                name: AttributeName::Html(HtmlAttr::class),
                value: None,
            })],
            data_attributes: vec![AttributeSelector(AttributePattern {
                name: AttributeName::Data("test"),
                value: None,
            })],
            ..Default::default()
        }),
    );
    test_ok(
        "img[src=\"image.png\"]:last-child",
        Some(CompoundSelector {
            element: Some(HtmlTag::img),
            attributes: vec![AttributeSelector(AttributePattern {
                name: AttributeName::Html(HtmlAttr::src),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Exact,
                    value: QuotedString("image.png".into()),
                    case: None,
                }),
            })],
            pseudo_classes: vec![PseudoClassSelector::LastChild],
            ..Default::default()
        }),
    );
    test_ok(
        "h1[id] ",
        Some(CompoundSelector {
            element: Some(HtmlTag::h1),
            attributes: vec![AttributeSelector(AttributePattern {
                name: AttributeName::Html(HtmlAttr::id),
                value: None,
            })],
            ..Default::default()
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<CompoundSelector>(string, expected);
    }

    test_err(
        ".",
        ParseError::Context(
            CompoundSelectorError::ClassFail(0).into(),
            ParseError::from(ClassSelectorError::MissingClass(0)).into(),
        ),
    );
    test_err(
        "#",
        ParseError::Context(
            CompoundSelectorError::IdFail(0).into(),
            ParseError::from(IdSelectorError::MissingId(0)).into(),
        ),
    );
    test_err(
        "[",
        ParseError::Context(
            CompoundSelectorError::AttributeFail(0).into(),
            ParseError::Context(
                AttributeSelectorError::ParseFail(0).into(),
                ParseError::from(ParenthesizedError::MissingContent(0)).into(),
            )
            .into(),
        ),
    );
    test_err(
        ":",
        ParseError::Context(
            CompoundSelectorError::PseudoClassFail(0).into(),
            ParseError::from(PseudoClassSelectorError::MissingKeyword(0)).into(),
        ),
    );
    test_err("[src]a", CompoundSelectorError::UnexpectedChar(5).into());
}

#[test]
fn test_compound_matching_ok() {
    fn test_match(selector: CompoundSelector) {
        let html = r#"<div id="main" class="red blue" data-foo="bar" title="main content">Some<article>long<span>paragraph</span></article></div><p>another graph</p>"#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // div

        assert!(selector.matches(&el));
    }

    // div#main
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        id: Some(IdSelector("main")),
        ..Default::default()
    });

    // div.red
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        id: None,
        classes: vec![ClassSelector("red")],
        ..Default::default()
    });

    // div[text]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        text: Some(AttributePattern {
            name: AttributeName::Text,
            value: None,
        }),
        ..Default::default()
    });

    // div[text$="graph"]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        text: Some(AttributePattern {
            name: AttributeName::Text,
            value: Some(AttributeValue {
                operator: AttributeOperator::Ends,
                value: QuotedString("graph".into()),
                case: None,
            }),
        }),
        ..Default::default()
    });

    // [title^="main"]
    test_match(CompoundSelector {
        attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::title),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("main".into()),
                case: None,
            }),
        })],
        ..Default::default()
    });

    // div[data-foo]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        data_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Data("foo"),
            value: None,
        })],
        ..Default::default()
    });

    // div.blue[title^="main"][data-foo][class="red"]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        classes: vec![ClassSelector("blue")],
        attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::title),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("main".into()),
                case: None,
            }),
        })],
        class_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("red".into()),
                case: None,
            }),
        })],
        data_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Data("foo"),
            value: None,
        })],
        ..Default::default()
    });
}

#[test]
fn test_compound_matching_err() {
    fn test_match(selector: CompoundSelector) {
        let html = r#"<div id="main" class="red blue" data-foo="bar" title="main content"></div>"#;

        let doc = HtmlDoc::parse(html).unwrap().dom();
        let el = doc.root();
        let el = el.first_child().unwrap(); // div

        assert!(!selector.matches(&el));
    }

    // div#footer
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        id: Some(IdSelector("footer")),
        ..Default::default()
    });

    // div[text]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        text: Some(AttributePattern {
            name: AttributeName::Text,
            value: None,
        }),
        ..Default::default()
    });

    // div.red.blue.green
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        classes: vec![
            ClassSelector("red"),
            ClassSelector("blue"),
            ClassSelector("green"),
        ],
        ..Default::default()
    });

    // [title^="content"]
    test_match(CompoundSelector {
        attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::title),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("content".into()),
                case: None,
            }),
        })],
        ..Default::default()
    });

    // div[data-bar]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        data_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Data("bar"),
            value: None,
        })],
        ..Default::default()
    });

    // section.blue[title^="main"][data-foo][class="red"]
    test_match(CompoundSelector {
        element: Some(HtmlTag::section),
        classes: vec![ClassSelector("blue")],
        attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::title),
            value: Some(AttributeValue {
                operator: AttributeOperator::Starts,
                value: QuotedString("main".into()),
                case: None,
            }),
        })],
        class_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Html(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("red".into()),
                case: None,
            }),
        })],
        data_attributes: vec![AttributeSelector(AttributePattern {
            name: AttributeName::Data("foo"),
            value: None,
        })],
        ..Default::default()
    });
}
