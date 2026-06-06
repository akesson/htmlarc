use super::{
    chars::Chars,
    dom::DomStack,
    tags::{parse_comment, parse_doctype, parse_end_tag, parse_start_tag},
};
use crate::{
    error::{Context, HtmlParseResult},
    html::HtmlTag,
};

#[cfg(test)]
use insta::assert_snapshot;

pub fn parse_doc<'a, Dom: DomStack<'a>>(dom: &mut Dom, chars: &mut Chars<'a>) -> HtmlParseResult<()> {
    let mut c = chars.current();
    let mut text_start: Option<usize> = None;
    loop {
        if c == '<' {
            if let Some(start) = text_start {
                dom.add_text_tag(HtmlTag::sys_text, chars.str(start..chars.index()));
                text_start = None;
            }

            let c = chars.next().unwrap();

            if c == '!' {
                let c = chars.next().unwrap();
                if c == '[' {
                    chars.next().unwrap();
                    // cdata, starts with <![CDATA[ and ends with ]]>
                    chars.assert_sequence(['C', 'D', 'A', 'T', 'A', '['])?;
                    chars
                        .find_sequence([']', ']', '>'])
                        .context("Could not find the end of a CDATA tag")?;
                    chars.assert_curr(|c| c == '>')?;
                } else if c == '-' {
                    // comments, starts with <!-- and ends with -->
                    parse_comment(dom, chars)?;
                } else if c == 'D' || c == 'd' {
                    // doctype starts with <!DOCTYPE and ends with >
                    parse_doctype(dom, chars).context("Could not parse the DOCTYPE tag")?;
                }
            } else if c == '/' {
                let tag = parse_end_tag(chars)?;
                dom.pop_tag(tag).with_context(|| chars.location_info())?;
            } else {
                parse_start_tag(dom, chars).context("Parsing start tag")?;
            }
        } else if text_start.is_none() {
            text_start = Some(chars.index());
        }

        let Some(next) = chars.next() else { break };
        c = next;
    }
    Ok(())
}

#[test]
fn test_parse_doc_one_normal_element() {
    let run = |s: &str, c: char| super::with_chars_and_dom(s, parse_doc, c);

    assert_snapshot!(run("<div></div> ", ' '), @"div");
    assert_snapshot!(run("<div class='hi'></div> ", ' '), @"div class='hi'");
    assert_snapshot!(run("<div /> ", ' '), @"div");
    // raw element
    assert_snapshot!(run("<style>hi</style> ", ' '), @r###"
    style
      text 'hi'
    "###);
    assert_snapshot!(run("<!--HI--> ", ' '), @"comment 'HI'");

    assert_snapshot!(run("<![CDATA[skip]]><div/> ", ' '), @"div");
}

#[test]
fn test_parse_doc_nested_elements() {
    let run = |s: &str, c: char| super::with_chars_and_dom(s, parse_doc, c);

    assert_snapshot!(run("<div><hr/></div> ", ' '), @r###"
      div
        hr
    "###);

    assert_snapshot!(run("<!DOCTYPE html><div>before<span><!--nope--><i/></span>after<b/>last</div> ", ' '), @r###"
    DOCTYPE html
    div
      text 'before'
      span
        comment 'nope'
        i
      text 'after'
      b
      text 'last'
    "###);
}

#[test]
fn test_parse_body_p_section() {
    let run = |s: &str, c: char| super::with_chars_and_dom(s, parse_doc, c);

    assert_snapshot!(run("<body><p><section></section></p></body> ", ' '), @r###"
    body
      p
        section
    "###);
}
