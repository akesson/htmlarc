use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        AttributeName, AttributeOperator, AttributePattern, AttributeValue, CaseIndicator, Context,
        IndexedError, ParseError, ParseResult, QuotedString,
        chars::CssChars,
        logging::debug,
        patterns::CssPattern,
        selectors::{
            pseudo_class::PseudoClassSelector,
            tag::{ExtTagSelector, TagSelector},
        },
    },
    dom::DomRead,
    html::{HtmlAttr, HtmlDoc, HtmlElement, HtmlTag},
    iters::DomIterator,
};

use super::{Selector, attribute::AttributeSelector, class::ClassSelector, id::IdSelector};
use crate::dom::DomView;

impl CompoundSelector<'_> {
    /// Bind every resolvable part of this compound to the document once (ADR 0002 §3): the
    /// id and attribute names/values to entry/name refs, the classes to `Sym`s, and the
    /// nested selectors of `:not`/`:is`/`:has`.
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        if let Some(ext) = &mut self.ext_element {
            ext.resolve(view);
        }
        if let Some(id) = &mut self.id {
            id.resolve(view);
        }
        for class in &mut self.classes {
            class.resolve(view);
        }
        for attr in &mut self.attributes {
            attr.resolve(view);
        }
        for pseudo in &mut self.pseudo_classes {
            pseudo.resolve(view);
        }
    }
}

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
    /// A standard type selector (`div`). Extended/custom-element selectors live in
    /// [`ext_element`](Self::ext_element); a compound holds at most one of the two.
    pub element: Option<HtmlTag>,
    /// An extended (custom/unknown) type selector (`my-widget`) — matched against the
    /// per-document extended-tag vocab (ADR 0002 §4), separate from the `HtmlTag` fast path.
    pub ext_element: Option<ExtTagSelector<'s>>,
    pub id: Option<IdSelector<'s>>,
    pub classes: Vec<ClassSelector<'s>>,
    /// Standard, `data-*`, and unknown attribute selectors — all matched against the unified
    /// attribute store. `[class]`/`[class=v]` go to `class_attributes` instead.
    pub attributes: Vec<AttributeSelector<'s>>,
    pub class_attributes: Vec<AttributeSelector<'s>>,
    pub pseudo_classes: Vec<PseudoClassSelector<'s>>,
    pub text: Option<AttributePattern<'s>>,
}

impl Display for CompoundSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(tag) = &self.element {
            write!(f, "{}", tag)?;
        }
        if let Some(ext) = &self.ext_element {
            write!(f, "{}", ext)?;
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
        // A standard tag binds the `HtmlTag` fast path; an extended/custom name binds the
        // `ext_element` slot (ADR 0002 §4). They are mutually exclusive — a compound has at
        // most one leading type selector.
        let mut compound = Self::default();
        match TagSelector::from_chars(chars)? {
            Some(TagSelector::Std(tag)) => compound.element = Some(tag),
            Some(TagSelector::Ext(ext)) => compound.ext_element = Some(ext),
            None => {}
        }

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
                            debug!("Duplicate id selector #{} at {}", c_id.name, index);
                            if c_id.name != id.name {
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
                        match attribute.pattern.name {
                            // `[class]`/`[class=v]` query the class tokens (see `eq_class`);
                            // every other name — std, `data-*`, unknown — queries the unified
                            // attribute store.
                            AttributeName::Std(HtmlAttr::class) => {
                                compound.class_attributes.push(attribute)
                            }
                            AttributeName::Std(_) | AttributeName::Ext(_) => {
                                compound.attributes.push(attribute)
                            }
                            AttributeName::Text => compound.text = Some(attribute.pattern),
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

    /// Whether this compound matches `el`. Binds the node's [`DomView`] once and delegates to the
    /// single [`matches_in_view`](Self::matches_in_view) body. The element accessors
    /// (`el.has_classes`, …) are each just `el.with_view(|v| v.<check>(el.index(), …))`, so matching
    /// *through them* would rebuild the view per check; binding it once here avoids that. This is
    /// the entry point for direct matches (`matches_css`) and the combinator `verify` path; the
    /// bound-walk hot path skips it and calls `matches_in_view` with a view bound for the walk.
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        el.dom().with_view(|view| self.matches_in_view(&view, el))
    }

    /// The single compound-matching body (ADR 0007). Integer topology/attribute checks read
    /// `view`; `text`/pseudo-class checks read `el` — `text` descends this node's subtree for its
    /// strings (factored into the `#[cold]` [`matches_text`](Self::matches_text)) and
    /// pseudo-classes navigate siblings, neither of which the (possibly text-empty) walk-bound view
    /// can serve. The integer checks read only the blob (node bytes, symbols, attr-value store),
    /// never the relocated text pool, so they are correct even against a view bound with an empty
    /// text source. `el.index()` locates the node within `view`.
    pub(crate) fn matches_in_view(&self, view: &DomView, el: &HtmlElement<impl DomRead>) -> bool {
        let index = el.index();

        if let Some(tag) = self.element
            && tag != view.nodes.tag(index)
        {
            return false;
        }

        if let Some(ext) = &self.ext_element
            && !view.matches_ext_tag(index, ext)
        {
            return false;
        }

        if let Some(id) = &self.id
            && !view.has_id_selector(index, id)
        {
            return false;
        }

        if let Some(text_pattern) = &self.text
            && !self.matches_text(text_pattern, el)
        {
            return false;
        }

        for pseudo_class in &self.pseudo_classes {
            if !pseudo_class.matches(el) {
                return false;
            }
        }

        if !self.attributes.is_empty() && !view.has_attributes(index, &self.attributes) {
            return false;
        }

        if !self.classes.is_empty() && !view.has_class_selectors(index, &self.classes) {
            return false;
        }

        if !self.class_attributes.is_empty() && !view.has_classes(index, &self.class_attributes) {
            return false;
        }

        true
    }

    /// The `[text]` / `[text*="…"]` content check — the rare, allocating branch (subtree text
    /// collect + optional case-fold), reading the document's strings via `el` (the walk-bound view
    /// may carry an empty text source). Returns whether the text constraint is satisfied.
    ///
    /// `#[cold]` is load-bearing, not decoration (measured): it discounts this call in
    /// [`matches_in_view`](Self::matches_in_view)'s inline cost, so that hot body stays cheap enough
    /// to inline into the select walk and lays the text branch out off the hot path. Without it the
    /// `tag` select regresses ~8–10%. (`#[inline(never)]` was tried and dropped — it gave no
    /// measurable gain on top of `#[cold]`, which already keeps a function this size out of line.)
    #[cold]
    fn matches_text(
        &self,
        text_pattern: &AttributePattern,
        el: &HtmlElement<impl DomRead>,
    ) -> bool {
        let mut text_iter = el.descendants().text_chars();
        if let Some(value) = &text_pattern.value {
            let (search, other) = if let Some(CaseIndicator::Insensitive) = &value.case {
                let search = value.value.0.to_lowercase();
                let other: String = text_iter.collect();
                (search, other)
            } else {
                (value.value.0.to_string(), text_iter.collect())
            };
            value.operator.matches(&search, &other)
        } else {
            text_iter.next().is_some()
        }
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
            id: Some(IdSelector::new("main")),
            ..Default::default()
        }),
    );
    test_ok(
        "div#footer.black",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            id: Some(IdSelector::new("footer")),
            classes: vec![ClassSelector::new("black")],
            ..Default::default()
        }),
    );
    test_ok(
        "div.red#header.green",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            id: Some(IdSelector::new("header")),
            classes: vec![ClassSelector::new("red"), ClassSelector::new("green")],
            ..Default::default()
        }),
    );
    test_ok(
        "div[class]",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            class_attributes: vec![AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::class),
                value: None,
            })],
            ..Default::default()
        }),
    );
    test_ok(
        "div.red[class][data-test]",
        Some(CompoundSelector {
            element: Some(HtmlTag::div),
            classes: vec![ClassSelector::new("red")],
            class_attributes: vec![AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::class),
                value: None,
            })],
            attributes: vec![AttributeSelector::new(AttributePattern {
                name: AttributeName::Ext("data-test"),
                value: None,
            })],
            ..Default::default()
        }),
    );
    test_ok(
        "img[src=\"image.png\"]:last-child",
        Some(CompoundSelector {
            element: Some(HtmlTag::img),
            attributes: vec![AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::src),
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
            attributes: vec![AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::id),
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
        id: Some(IdSelector::new("main")),
        ..Default::default()
    });

    // div.red
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        id: None,
        classes: vec![ClassSelector::new("red")],
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
        attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::title),
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
        attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Ext("data-foo"),
            value: None,
        })],
        ..Default::default()
    });

    // div.blue[title^="main"][data-foo][class="red"]
    test_match(CompoundSelector {
        element: Some(HtmlTag::div),
        classes: vec![ClassSelector::new("blue")],
        attributes: vec![
            AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::title),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Starts,
                    value: QuotedString("main".into()),
                    case: None,
                }),
            }),
            AttributeSelector::new(AttributePattern {
                name: AttributeName::Ext("data-foo"),
                value: None,
            }),
        ],
        class_attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("red".into()),
                case: None,
            }),
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
        id: Some(IdSelector::new("footer")),
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
            ClassSelector::new("red"),
            ClassSelector::new("blue"),
            ClassSelector::new("green"),
        ],
        ..Default::default()
    });

    // [title^="content"]
    test_match(CompoundSelector {
        attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::title),
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
        attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Ext("data-bar"),
            value: None,
        })],
        ..Default::default()
    });

    // section.blue[title^="main"][data-foo][class="red"]
    test_match(CompoundSelector {
        element: Some(HtmlTag::section),
        classes: vec![ClassSelector::new("blue")],
        attributes: vec![
            AttributeSelector::new(AttributePattern {
                name: AttributeName::Std(HtmlAttr::title),
                value: Some(AttributeValue {
                    operator: AttributeOperator::Starts,
                    value: QuotedString("main".into()),
                    case: None,
                }),
            }),
            AttributeSelector::new(AttributePattern {
                name: AttributeName::Ext("data-foo"),
                value: None,
            }),
        ],
        class_attributes: vec![AttributeSelector::new(AttributePattern {
            name: AttributeName::Std(HtmlAttr::class),
            value: Some(AttributeValue {
                operator: AttributeOperator::Exact,
                value: QuotedString("red".into()),
                case: None,
            }),
        })],
        ..Default::default()
    });
}
