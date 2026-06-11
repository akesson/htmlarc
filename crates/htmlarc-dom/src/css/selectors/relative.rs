use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{Combinator, CssPattern},
    },
    dom::DomRead,
    html::HtmlElement,
};

use super::{Selector, compound::CompoundSelector};
use crate::dom::DomView;

impl RelativeSelector<'_> {
    pub(crate) fn resolve(&mut self, view: DomView<'_>) {
        self.selector.resolve(view);
    }
}

#[derive(Debug, Error)]
pub enum RelativeSelectorError {
    #[error("Missing relative selector at {0}")]
    MissingSelector(usize),
    #[error("Failed to parse relative selector at {0}")]
    SelectorFail(usize),
}

impl From<RelativeSelectorError> for ParseError {
    fn from(val: RelativeSelectorError) -> Self {
        val.into_parse_error()
    }
}

impl RelativeSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for RelativeSelectorError {
    fn index(&self) -> usize {
        match *self {
            RelativeSelectorError::MissingSelector(index) => index,
            RelativeSelectorError::SelectorFail(index) => index,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelativeSelector<'s> {
    pub combinator: Combinator,
    pub selector: CompoundSelector<'s>,
}

impl Display for RelativeSelector<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}", self.combinator, self.selector)
    }
}

impl<'s> CssPattern<'s> for RelativeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl<'s> Selector<'s> for RelativeSelector<'s> {
    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.matches(el)
    }
}

impl<'s> RelativeSelector<'s> {
    fn from_chars(chars: &mut CssChars<'s>) -> ParseResult<Option<Self>> {
        let Some((_, _)) = chars.current() else {
            debug!("No relative selector found at {}", chars.last_index());
            return Ok(None);
        };

        debug!("Parsing combinator at {}", chars.last_index());
        let combinator = Combinator::from_chars(chars)?;

        let selector_index = chars.last_index();

        debug!("Parsing compound selector at {}", selector_index);
        if let Some(selector) = CompoundSelector::from_chars(chars)
            .context(RelativeSelectorError::SelectorFail(selector_index))?
        {
            chars.skip_spaces();
            debug!("Parsed relative selector at {}", chars.last_index());
            Ok(Some(Self {
                combinator: combinator.unwrap_or_default(),
                selector,
            }))
        } else if combinator.is_none() {
            debug!("Empty relative selector found at {}", selector_index);
            Ok(None)
        } else {
            Err(RelativeSelectorError::MissingSelector(selector_index).into())
        }
    }

    fn matches(&self, el: &HtmlElement<impl DomRead>) -> bool {
        self.selector.matches(el)
    }
}

#[test]
fn test_parse_relative_selector() {
    use crate::{
        css::{helpers::test_ok, selectors::compound::CompoundSelectorError},
        html::HtmlTag,
    };

    test_ok("", None::<RelativeSelector>);
    test_ok(
        "div",
        Some(RelativeSelector {
            combinator: Combinator::Descendant,
            selector: CompoundSelector {
                element: Some(HtmlTag::div),
                ..Default::default()
            },
        }),
    );
    test_ok(
        " >span",
        Some(RelativeSelector {
            combinator: Combinator::Child,
            selector: CompoundSelector {
                element: Some(HtmlTag::span),
                ..Default::default()
            },
        }),
    );
    test_ok(
        "+  p",
        Some(RelativeSelector {
            combinator: Combinator::NextSibling,
            selector: CompoundSelector {
                element: Some(HtmlTag::p),
                ..Default::default()
            },
        }),
    );

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<RelativeSelector>(string, expected);
    }

    test_err(">", RelativeSelectorError::MissingSelector(0).into());
    test_err(
        "+[src]h1",
        ParseError::Context(
            RelativeSelectorError::SelectorFail(1).into(),
            ParseError::from(CompoundSelectorError::UnexpectedChar(6)).into(),
        ),
    );
}
