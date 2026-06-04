//!
//! Based on: https://html.spec.whatwg.org/multipage/syntax.html#attributes-2
//!
use super::{DomStack, chars::Chars};
use crate::html::HtmlAttr;
use crate::{Context, ParseResult};

/// Pre-condition: chars is positioned on the first character after the tag.
///
/// Post-condition: chars is positioned on the first character after the
/// attributes section, which is one of `/` `>`
pub fn parse_attributes<'a, Dom: DomStack<'a>>(
    dom: &mut Dom,
    chars: &mut Chars<'a>,
) -> ParseResult<()> {
    loop {
        chars.skip_whitespaces();
        let mut c = chars.current();
        if c.is_ascii_alphanumeric() {
            parse_attribute(dom, chars)?;
            c = chars.current();
        }

        if c == '>' || c == '/' {
            return Ok(());
        } else if c.is_ascii_alphanumeric() {
            // do nothing, sometimes a quoted attribute is followed
            // immediately by another attribute
        } else if chars.next().is_none() {
            return Err(chars.err("Couldn't find the end of the tag"));
        }
    }
}

/// Pre-condition: chars is positioned on the first character of the attribute name.
///
/// Post-condition: chars is positioned on the first character after the attribute.
fn parse_attribute<'a, Dom: DomStack<'a>>(dom: &mut Dom, chars: &mut Chars<'a>) -> ParseResult<()> {
    let start = chars.index();
    chars
        .find(attr_name_end)
        .context("The end of the attribute name not found")?;

    let name = chars.str_from(start);

    let attribute: HtmlAttr = match name.try_into() {
        Ok(a) => a,
        Err(_) if name.starts_with("data-") => HtmlAttr::sys_deleted,
        Err(_) => return Err(chars.err(format!("Not a valid attribute: '{name}'"))),
    };
    // let attribute: HtmlAttr = name
    //     .try_into()
    //     .map_err(|_| chars.err(format!("Not a valid attribute: '{name}'")))?;

    chars.skip_whitespaces();

    let c = chars.current();
    if c == '=' {
        let c = chars.next_skip_whitespaces().unwrap();
        let value = if c == '"' {
            let start = chars.next_index().unwrap();
            let end = chars
                .find(|c| c == '"')
                .context("Could not find closing double quote")?;
            chars.next();
            chars.str(start..end)
        } else if c == '\'' {
            let start = chars.next_index().unwrap();
            let end = chars
                .find(|c| c == '\'')
                .context("Could not find closing single quote")?;
            chars.next();
            chars.str(start..end)
        } else {
            let start = chars.index();
            chars
                .str_until(start, |c| c.is_whitespace() || c == '>')
                .context("Could not find the end of the attribute")?
        };
        if attribute == HtmlAttr::sys_deleted {
            let tag = name.replace("data-", "");
            dom.add_data_attribute(&tag, value);
        } else {
            dom.add_attribute_and_value(attribute, value);
        }
    } else {
        dom.add_attribute_and_value(attribute, "");
    };
    Ok(())
}

fn attr_name_end(c: char) -> bool {
    c.is_whitespace() || c == '=' || c == '"' || c == '\'' || c == '/' || c == '>'
}

#[cfg(test)]
use super::testdom::TestDom;
#[cfg(test)]
use crate::html::HtmlTag;
#[cfg(test)]
use insta::assert_snapshot;

#[track_caller] // Will show the location of the caller in test failure messages
#[cfg(test)]
fn parse_all(html: &str) -> String {
    let mut dom = TestDom::new_with(HtmlTag::div);
    let mut chars = Chars::new(html);
    parse_attributes(&mut dom, &mut chars).unwrap();
    dom.pop_tag(HtmlTag::div).unwrap();
    format!("{}  chars({})", dom.to_string().trim_end(), chars.current())
}

#[track_caller] // Will show the location of the caller in test failure messages
#[cfg(test)]
fn parse_one(html: &str) -> String {
    let mut dom = TestDom::new_with(HtmlTag::div);
    let mut chars = Chars::new(html);
    parse_attribute(&mut dom, &mut chars).unwrap();
    dom.pop_tag(HtmlTag::div).unwrap();
    format!("{}  chars({})", dom.to_string().trim(), chars.current())
}

#[test]
fn test_single_attribute_no_value() {
    assert_snapshot!(parse_one("disabled>"), @"div disabled  chars(>)");
    assert_snapshot!(parse_one("disabled >"), @"div disabled  chars(>)");
    assert_snapshot!(parse_one("disabled  >"), @"div disabled  chars(>)");
    assert_snapshot!(parse_one(r##"disabled closed>"##), @"div disabled  chars(c)");
    assert_snapshot!(parse_one(r##"disabled  closed>"##), @"div disabled  chars(c)");
    assert_snapshot!(parse_one(r##"disabled/>"##), @"div disabled  chars(/)");
    assert_snapshot!(parse_one(r##"disabled />"##), @"div disabled  chars(/)");
    assert_snapshot!(parse_one(r##"disabled  />"##), @"div disabled  chars(/)");
}

#[test]
fn test_single_attribute_with_double_quoted_value() {
    assert_snapshot!(parse_one(r#"title="hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title ="hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title  ="hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title="hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title= "hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title=  "hi">"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title="hi"disabled>"#), @"div title='hi'  chars(d)");

    assert_snapshot!(parse_one(r#"title="hi" >"#), @"div title='hi'  chars( )");
    assert_snapshot!(parse_one(r#"title="hi"  >"#), @"div title='hi'  chars( )");
    assert_snapshot!(parse_one(r#"title="hi"/>"#), @"div title='hi'  chars(/)");
    assert_snapshot!(parse_one(r#"title="hi" />"#), @"div title='hi'  chars( )");
    assert_snapshot!(parse_one(r#"title="hi"  />"#), @"div title='hi'  chars( )");
}

#[test]
fn test_single_attribute_with_single_quoted_value() {
    assert_snapshot!(parse_one(r#"title='hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title ='hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title  ='hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title='hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title= 'hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title=  'hi'>"#), @"div title='hi'  chars(>)");
    assert_snapshot!(parse_one(r#"title='h"i'>"#), @r###"div title='h"i'  chars(>)"###);
}

#[test]
fn test_multiple_attributes() {
    assert_snapshot!(parse_all(r#"disabled>"#), @"div disabled  chars(>)");
    assert_snapshot!(parse_all(r#"disabled >"#), @"div disabled  chars(>)");
    assert_snapshot!(parse_all(r#"disabled  >"#), @"div disabled  chars(>)");

    assert_snapshot!(parse_all(r#"class="some">"#), @"div class='some'  chars(>)");
    assert_snapshot!(parse_all(r#"class="some" >"#), @"div class='some'  chars(>)");
    assert_snapshot!(parse_all(r#"class="some"  >"#), @"div class='some'  chars(>)");

    assert_snapshot!(parse_all(r#"class="some" />"#), @"div class='some'  chars(/)");
    assert_snapshot!(parse_all(r#"class="some" />"#), @"div class='some'  chars(/)");
    assert_snapshot!(parse_all(r#"class="some"  />"#), @"div class='some'  chars(/)");

    assert_snapshot!(parse_all(r#"class="some" disabled />"#), @"div class='some' disabled  chars(/)");

    assert_snapshot!(parse_all(r#"class="some"title="hi" disabled/>"#), @"div class='some' title='hi' disabled  chars(/)");
    assert_snapshot!(parse_all(r#"class="some" title="hi" disabled/>"#), @"div class='some' title='hi' disabled  chars(/)");
    assert_snapshot!(parse_all(r#"class="some"    title="hi"    disabled   />"#), @"div class='some' title='hi' disabled  chars(/)");
    assert_snapshot!(parse_all(r#" class="some"    title="hi"    disabled  />"#), @"div class='some' title='hi' disabled  chars(/)");
    assert_snapshot!(parse_all(r#"  class="some"    title="hi"    disabled />"#), @"div class='some' title='hi' disabled  chars(/)");
}
