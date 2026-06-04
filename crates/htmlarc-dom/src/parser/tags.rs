use std::ops::Range;

use super::{attributes::parse_attributes, chars::Chars, dom::DomStack};
use crate::error::ParseResult;
use crate::html::{HtmlAttr, HtmlTag};
#[cfg(test)]
use insta::assert_snapshot;

/// Pre-condition: chars at the first '-' in <!--
///
/// Post-condition: chars at the closing '>'
pub fn parse_comment<'a, Dom: DomStack<'a>>(dom: &mut Dom, chars: &mut Chars) -> ParseResult<()> {
    chars.assert_next(|c| c == '-')?;
    chars.next().unwrap();
    let start = chars.index();
    let end = chars.find_sequence(['-', '-', '>'])?;
    dom.add_text_tag(HtmlTag::sys_comment, chars.str(start..end));
    Ok(())
}

#[test]
fn test_parse_comment() {
    let run = |s: &str| super::with_chars_and_dom(s, parse_comment, '>');

    assert_snapshot!(run("--hi-->"), @"comment 'hi'");
    assert_snapshot!(run("--a&<->b-->"), @"comment 'a&<->b'");
    assert_snapshot!(run("---->"), @"comment ''");
}

/// Pre-condition: chars at the first char of the DOMTYPE, 'D' or 'd'
///
/// Post-condition: chars at the closing '>'
pub(super) fn parse_doctype<'a, Dom: DomStack<'a>>(
    dom: &mut Dom,
    chars: &mut Chars,
) -> ParseResult<()> {
    chars.assert_next(|c| c == 'o' || c == 'O')?;
    chars.assert_next(|c| c == 'c' || c == 'C')?;
    chars.assert_next(|c| c == 't' || c == 'T')?;
    chars.assert_next(|c| c == 'y' || c == 'Y')?;
    chars.assert_next(|c| c == 'p' || c == 'P')?;
    chars.assert_next(|c| c == 'e' || c == 'E')?;
    chars.next();
    chars.skip_whitespaces();
    chars.assert_curr(|c| c == 'h' || c == 'H')?;
    chars.assert_next(|c| c == 't' || c == 'T')?;
    chars.assert_next(|c| c == 'm' || c == 'M')?;
    chars.assert_next(|c| c == 'l' || c == 'L')?;
    chars.next();
    chars.skip_whitespaces();
    chars.assert_curr(|c| c == '>')?;
    dom.push_tag(HtmlTag::DOCTYPE);
    dom.add_attribute_and_value(HtmlAttr::html, "");
    dom.pop_tag(HtmlTag::DOCTYPE)?;
    Ok(())
}

#[test]
fn test_parse_doctype() {
    let run = |s: &str| super::with_chars_and_dom(s, parse_doctype, '>');

    assert_snapshot!(run("doctype html>"), @"DOCTYPE html");
    assert_snapshot!(run("DOCTYPE HTML>"), @"DOCTYPE html");
}

/// Pre-condition: chars positioned at the first character of the tag
///
/// Post-condition: chars positioned at the last character of the tag, i.e. '>'
pub(super) fn parse_start_tag<'a, Dom: DomStack<'a>>(
    dom: &mut Dom,
    chars: &mut Chars<'a>,
) -> ParseResult<()> {
    let tag = parse_tag(chars)?;

    if foreign_element(tag, chars)? {
        // do nothing, foreign elements are skipped
    } else {
        dom.push_tag(tag);
        parse_attributes(dom, chars)?;
        let closed = to_end_of_tag(chars)?;

        if let Some(text_range) = raw_element(tag, chars)? {
            dom.add_text_tag(HtmlTag::sys_text, chars.str(text_range));
            dom.pop_tag(tag)?;
        } else if closed || tag.is_void_element() {
            dom.pop_tag(tag)?;
        }
    }
    Ok(())
}

#[test]
fn test_parse_start_tag() {
    let run = |s: &str| super::with_chars_and_dom(s, parse_start_tag, '>');

    // normal tag
    assert_snapshot!(run("div> "), @"div");
    assert_snapshot!(run("div disabled> "), @"div disabled");
    assert_snapshot!(run(r#"div class="tst"> "#), @"div class='tst'");
    assert_snapshot!(run(r#"div class="tst" hidden> "#), @"div class='tst' hidden");

    // raw element
    assert_snapshot!(run("script>text</hr></script> "), @r###"
    script
      text 'text</hr>'
    "###);

    // foreign element
    assert_snapshot!(run("svg>whatever</svg> "), @"");
}

/// Pre-condition: chars positioned at '/'
///
/// Parses the end tag and throws an error if it doesn't correspond to the latest
/// in the domstack.
///
/// Post-condition: chars positioned at '>'
pub(super) fn parse_end_tag(chars: &mut Chars) -> ParseResult<HtmlTag> {
    chars.next().unwrap();
    let start = chars.index();
    let end = chars.find(|c| c.is_whitespace() || c == '>')?;
    let tag_str = chars.str(start..end);
    let tag: HtmlTag = tag_str
        .try_into()
        .map_err(|_| chars.err(format!("Not a valid tag: '{tag_str}'")))?;
    if tag.as_str() != tag_str {
        return Err(chars.err(format!(
            "Expected to close tag <{tag}> but found <{tag_str}>"
        )));
    }
    chars.skip_whitespaces();

    if chars.current() != '>' {
        Err(chars.err(format!(
            "An end tag must terminate with a '>' char, not {}",
            chars.current()
        )))
    } else {
        Ok(tag)
    }
}

#[test]
fn test_parse_end_tag() {
    let run = |s: &str, c: char| -> ParseResult<HtmlTag> {
        super::with_chars_check_last(s, parse_end_tag, c)
    };
    assert_eq!(run("/div> ", '>'), Ok(HtmlTag::div));
    assert_eq!(run("/div > ", '>'), Ok(HtmlTag::div));
    assert_eq!(run("/div  > ", '>'), Ok(HtmlTag::div));

    assert!(run("/div $> ", '>').is_err());
}

/// Pre-condition: chars at the start tag's closing '>'
///
/// If the tag is a raw one, then returns the range of the text content
/// with chars positioned at the end tags closing '>'.
/// Otherwise return none with chars at the same position as when the function was called
fn raw_element(tag: HtmlTag, chars: &mut Chars) -> ParseResult<Option<Range<usize>>> {
    let range = match tag {
        // raw text including escapable
        HtmlTag::script => raw_elem_seq(chars, ['<', '/', 's', 'c', 'r', 'i', 'p', 't'])?,
        HtmlTag::style => raw_elem_seq(chars, ['<', '/', 's', 't', 'y', 'l', 'e'])?,
        HtmlTag::title => raw_elem_seq(chars, ['<', '/', 't', 'i', 't', 'l', 'e'])?,
        HtmlTag::textarea => {
            raw_elem_seq(chars, ['<', '/', 't', 'e', 'x', 't', 'a', 'r', 'e', 'a'])?
        }
        _ => return Ok(None),
    };
    chars.next().unwrap();

    to_end_of_tag(chars)?;
    Ok(Some(range))
}

fn raw_elem_seq<const N: usize>(chars: &mut Chars, seq: [char; N]) -> ParseResult<Range<usize>> {
    chars.next();
    let start = chars.index();
    let end = chars.find_sequence(seq)?;
    Ok(start..end)
}

#[cfg(test)]
fn run_raw_element(string: &str, tag: HtmlTag, c: char) -> String {
    let mut chars = Chars::new(string);
    let range = match raw_element(tag, &mut chars) {
        Ok(range) => range.unwrap_or(0..0),
        Err(e) => return e.to_string(),
    };
    if c != chars.current() {
        format!("Expected chars to be at {c} but is at {}", chars.current())
    } else {
        chars.str(range).to_string()
    }
}

#[test]
fn test_raw_element() {
    use HtmlTag::{div, script, style, textarea, title};
    assert_eq!(run_raw_element(">text</script> ", script, '>'), "text");
    assert_eq!(run_raw_element(">text</style> ", style, '>'), "text");
    assert_eq!(run_raw_element(">text</title>", title, '>'), "text");
    assert_eq!(run_raw_element(">text</textarea>", textarea, '>'), "text");

    assert_eq!(run_raw_element(">text</title>", div, '>'), "");

    assert_eq!(
        run_raw_element(">text<ignored/></script>", script, '>'),
        "text<ignored/>"
    );
}

/// Foreign elements (svg & math) are skipped: this will find the end-tag of the element
/// and forward to the ending '>'.
///
/// Returns true if the provided tag is a foreign element.
fn foreign_element(tag: HtmlTag, chars: &mut Chars) -> ParseResult<bool> {
    let _ = match tag {
        // foreign elements svg and math
        HtmlTag::svg => chars.find_sequence(['<', '/', 's', 'v', 'g'])?,
        HtmlTag::math => chars.find_sequence(['<', '/', 'm', 'a', 't', 'h'])?,
        _ => return Ok(false),
    };
    chars.next().unwrap();
    to_end_of_tag(chars)?;
    Ok(true)
}

#[test]
fn test_foreign_element() {
    let run = |string: &str, tag: HtmlTag| -> ParseResult<bool> {
        super::with_chars_check_last(string, |chars| foreign_element(tag, chars), '>')
    };

    assert_eq!(run("svg> </svg>", HtmlTag::svg), Ok(true));
    assert_eq!(run("svg> </svg >", HtmlTag::svg), Ok(true));
    assert_eq!(run("svg> </svg  >", HtmlTag::svg), Ok(true));
    assert_eq!(run("whatever </svg>", HtmlTag::svg), Ok(true));
    assert_eq!(run("</math>", HtmlTag::math), Ok(true));

    assert!(foreign_element(HtmlTag::svg, &mut Chars::new("hi")).is_err());
}

/// Skip any spaces and '/' and returns if the tag was closed
/// with chars at the last '>'
fn to_end_of_tag(chars: &mut Chars) -> ParseResult<bool> {
    chars.skip_whitespaces();
    let mut c = chars.current();
    let mut closed = false;
    if c == '/' {
        c = chars.next().unwrap();
        closed = true;
    }
    if c != '>' {
        return Err(chars.err(format!(
            "Invalid end of tag, a '/' should be followed by a '>', not {c}"
        )));
    }
    Ok(closed)
}

#[test]
fn test_to_end_of_tag() {
    use super::with_chars_check_last as run;
    assert_eq!(run("> ", to_end_of_tag, '>'), Ok(false));
    assert_eq!(run(" > ", to_end_of_tag, '>'), Ok(false));
    assert_eq!(run("  > ", to_end_of_tag, '>'), Ok(false));

    assert_eq!(run("/> ", to_end_of_tag, '>'), Ok(true));
    assert_eq!(run(" /> ", to_end_of_tag, '>'), Ok(true));
    assert_eq!(run("  /> ", to_end_of_tag, '>'), Ok(true));

    assert!(super::with_chars("/ ", to_end_of_tag).is_err());
}

/// Pre-condition: chars positioned at the first character of the tag
///
/// Post-condition: chars positioned at the first character after the tag
fn parse_tag(chars: &mut Chars) -> ParseResult<HtmlTag> {
    let start = chars.index();
    let end = chars.find(|c| c.is_whitespace() || c == '/' || c == '>')?;
    let tag_str = chars.str(start..end);

    tag_str
        .try_into()
        .map_err(|_| chars.err(format!("Not a valid tag: '{tag_str}'")))
}

#[test]
fn test_parse_tag() {
    use super::with_chars_check_last as run;

    assert_eq!(run("div ", parse_tag, ' '), Ok(HtmlTag::div));
    assert_eq!(run("div ", parse_tag, ' '), Ok(HtmlTag::div));
    assert_eq!(run("div>", parse_tag, '>'), Ok(HtmlTag::div));
    assert_eq!(run("div/>", parse_tag, '/'), Ok(HtmlTag::div));
    assert_eq!(run("div />", parse_tag, ' '), Ok(HtmlTag::div));
}
