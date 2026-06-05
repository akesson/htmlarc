use crate::{css::Diagnostic, html::HtmlTag};

use super::{
    ClassSelector, Combinator, ComplexSelector, CompoundSelector, IdSelector, ParseResult,
    RelativeSelector, SelectorList, chars::CssChars, patterns::CssPattern,
};

pub fn parse_css(string: &str) -> ParseResult<SelectorList<'_>> {
    let mut chars = CssChars::new(string);

    SelectorList::from_chars(&mut chars)?.ok_or(crate::css::ParseError::EmptySelector)
}

#[test]
fn test_parse_round_trip() {
    let css = "div#header.blue > p, h1.title + span, a:first-child.link input[type='text']";

    let selectors = parse_css(css).unwrap();

    assert_eq!(css, selectors.to_string());

    let spaced_css =
        "div#header.blue    > p, h1.title   + span   , a:first-child.link  input[type='text']";
    let spaced_selectors = parse_css(css).unwrap();

    assert_eq!(spaced_selectors.to_string(), selectors.to_string());

    let css = r#"a.button.active + .tooltip, [method='POST' s] ~ .message, :first-child.card > p:nth-child(2n+1).details"#;
    let selectors = parse_css(css).unwrap();

    assert_eq!(css, selectors.to_string());
}

#[test]
fn test_parse_error() {
    use insta::assert_snapshot;

    let css = "div#header.blue > p, h1.title + span  a:firdt-child.link input[type=\"text\"]";
    let diagnosis = parse_css(css).unwrap_err().diagnosis(css);
    println!("{}", diagnosis);
    assert_snapshot!(diagnosis);

    let css = "div#header.blue > p, h1.title + span  a:first-child.link input[type=\"text\"]img";
    let diagnosis = parse_css(css).unwrap_err().diagnosis(css);
    println!("{}", diagnosis);
    assert_snapshot!(diagnosis);

    let css = r#"a.button.active + .tooltip, [method="POST" s] ~ .message, :first-child.card > p:nth-child(2n+1a).details"#;
    let diagnosis = parse_css(css).unwrap_err().diagnosis(css);
    println!("{}", diagnosis);
    assert_snapshot!(diagnosis);
}
