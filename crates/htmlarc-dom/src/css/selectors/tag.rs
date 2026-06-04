use std::fmt::Display;

use thiserror::Error;

use crate::{
    css::{
        Context, IndexedError, ParseError, ParseResult,
        chars::CssChars,
        logging::debug,
        patterns::{CssChar, CssPattern, TextPattern},
    },
    html::HtmlTag,
};

#[derive(Debug, Error)]
pub enum TagSelectorError {
    #[error("Failed to parse tag selector at {0}")]
    ParseFail(usize),
    #[error("Invalid tag selector at {0}")]
    Invalid(usize),
}

impl From<TagSelectorError> for ParseError {
    fn from(value: TagSelectorError) -> Self {
        value.into_parse_error()
    }
}

impl TagSelectorError {
    pub fn into_parse_error(self) -> ParseError {
        ParseError::new(self)
    }
}

impl IndexedError for TagSelectorError {
    fn index(&self) -> usize {
        match *self {
            TagSelectorError::ParseFail(index) => index,
            TagSelectorError::Invalid(index) => index,
        }
    }
}

#[derive(Debug)]
pub struct TagSelector(HtmlTag);

impl Display for TagSelector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl CssPattern<'_> for TagSelector {
    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl TagSelector {
    pub fn inner(self) -> HtmlTag {
        self.0
    }

    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        let Some((tag_index, _)) = chars.current() else {
            debug!("No tag selector found at {}", chars.last_index());
            return Ok(None);
        };

        let pattern = TextPattern::default()
            .allow_alphabetic()
            .allow_numeric()
            .start_with(CssChar::Alphabetic)
            .allow_special('-')
            .allow_special('_')
            .stop_at('[')
            .stop_at('#')
            .stop_at(':')
            .stop_at('.')
            .stop_at(' ')
            .stop_at('\n');

        debug!("Parsing tag name at {}", tag_index);
        if let Some(tag) = pattern
            .validate(chars)
            .context(TagSelectorError::ParseFail(tag_index))?
        {
            debug!("Checking if tag exists at {}", chars.last_index());
            if let Ok(htmltag) = HtmlTag::try_from(tag) {
                debug!("Parsed tag selector at {}", chars.last_index());
                Ok(Some(Self(htmltag)))
            } else {
                Err(TagSelectorError::Invalid(tag_index).into())
            }
        } else {
            debug!("No tag selector found at {}", tag_index);
            Ok(None)
        }
    }
}

#[test]
fn test_parse_tag_selector() {
    use crate::css::{ParseError, helpers::test_ok, patterns::TextPatternError};

    test_ok("", None::<TagSelector>);
    test_ok("div", Some(TagSelector(HtmlTag::div)));
    test_ok("h1", Some(TagSelector(HtmlTag::h1)));
    test_ok("figure-inline", Some(TagSelector(HtmlTag::figure_inline)));
    test_ok("span.inline", Some(TagSelector(HtmlTag::span)));

    fn test_err(string: &str, expected: ParseError) {
        crate::css::helpers::test_err::<TagSelector>(string, expected);
    }

    test_err("dib", TagSelectorError::Invalid(0).into());
    test_err(
        "1div",
        ParseError::new(TextPatternError::StartsWith(0, '1'))
            .context(TagSelectorError::ParseFail(0)),
    );
}
