use std::fmt::Display;

use crate::css::{ParseResult, chars::CssChars, logging::debug};

use super::CssPattern;

/// [mdn: Combinators](https://developer.mozilla.org/en-US/docs/Web/CSS/CSS_selectors/Selectors_and_combinators#combinatorshttps://developer.mozilla.org/en-US/docs/Web/CSS/CSS_selectors/Selectors_and_combinators#combinators)
#[derive(Debug, Eq, PartialEq, Clone, Copy, Default)]
pub enum Combinator {
    /// ' '
    #[default]
    Descendant,
    /// '>'
    Child,
    /// '~'
    SubsequentSibling,
    /// '+'
    NextSibling,
}

impl Display for Combinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Combinator::Descendant => write!(f, " "),
            Combinator::Child => write!(f, "> "),
            Combinator::SubsequentSibling => write!(f, "~ "),
            Combinator::NextSibling => write!(f, "+ "),
        }
    }
}

impl CssPattern<'_> for Combinator {
    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        Self::from_chars(chars)
    }
}

impl Combinator {
    fn from_chars(chars: &mut CssChars<'_>) -> ParseResult<Option<Self>> {
        let Some((_, c)) = chars.current() else {
            debug!("No combinator found at {}", chars.last_index());
            return Ok(None);
        };

        let mut descendant = false;

        if c.is_whitespace() {
            debug!("Skiping spaces");
            descendant = true;
            chars.skip_spaces();
        }

        let Some((_, c)) = chars.current() else {
            debug!("No more characters found at {}", chars.last_index());
            return Ok(None);
        };

        match c {
            '>' => {
                chars.next();
                chars.skip_spaces();
                debug!("Found child combinator");
                Ok(Some(Combinator::Child))
            }
            '~' => {
                chars.next();
                chars.skip_spaces();
                debug!("Found subsequent sibling combinator");
                Ok(Some(Combinator::SubsequentSibling))
            }
            '+' => {
                chars.next();
                chars.skip_spaces();
                debug!("Found next sibling combinator");
                Ok(Some(Combinator::NextSibling))
            }
            _ => {
                if descendant {
                    debug!("Found descendant combinator");
                    Ok(Some(Combinator::Descendant))
                } else {
                    debug!("No combinator found at {}", chars.last_index());
                    Ok(None)
                }
            }
        }
    }
}

#[test]
fn test_parse_combinator() {
    use crate::css::helpers::test_ok;

    test_ok("", None::<Combinator>);
    test_ok(" ", None::<Combinator>);
    test_ok("div", None::<Combinator>);
    test_ok("    a", Some(Combinator::Descendant));
    test_ok("   >", Some(Combinator::Child));
    test_ok("   ~", Some(Combinator::SubsequentSibling));
    test_ok("+", Some(Combinator::NextSibling));
}
