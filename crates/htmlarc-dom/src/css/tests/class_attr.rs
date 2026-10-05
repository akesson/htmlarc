//! `[class…]` attribute selectors test the whole `class` value, as the spec says. Expected
//! ids were cross-checked against lxml (raw value) and bs4 (whitespace-normalized value).

use crate::{dom::DomRead, html::HtmlDoc};

const HTML: &str = "<div id=a class=\"foo bar\"></div><div id=b class=\"foobar\"></div>\
    <div id=c class=\"xfoo\"></div><div id=d class=\"Foo\"></div>\
    <div id=e class=\"bar  foo\"></div><div id=f class=\" foo\"></div>\
    <div id=g class=\"foo \"></div><div id=h class=\"\"></div><div id=i class=\"   \"></div>\
    <div id=j class=\"foo-x bar\"></div><div id=k class=\"foo\tbar\"></div><div id=l></div>\
    <div id=m class=\"foo\"></div><div id=n class=\"-x y\"></div>";

/// The ids of the matched elements, concatenated.
fn ids(css: &str) -> String {
    let doc = HtmlDoc::parse(HTML).unwrap();
    doc.dom()
        .root()
        .select_css(css)
        .unwrap()
        .map(|el| el.with_id(|id| id.unwrap_or("?").to_string()))
        .collect()
}

#[test]
fn class_list_word_match_is_exact() {
    // lxml and bs4 agree on all of these.
    assert_eq!(ids("[class]"), "abcdefghijkmn");
    assert_eq!(ids(r#"[class~="foo"]"#), "aefgkm");
    assert_eq!(ids(r#"[class~="foo"]"#), ids(".foo"));
    assert_eq!(ids(r#"[class~="oo"]"#), "");
    assert_eq!(ids(r#"[class~=""]"#), "");
    assert_eq!(ids(r#"[class~="foo bar"]"#), "");
    assert_eq!(ids(r#"[class~="FOO" i]"#), "adefgkm");
}

#[test]
fn class_operators_test_the_whole_value() {
    // lxml and bs4 agree on all of these.
    assert_eq!(ids(r#"[class*="oo"]"#), "abcdefgjkm");
    assert_eq!(ids(r#"[class^=""]"#), "");
    assert_eq!(ids(r#"[class$=""]"#), "");
    assert_eq!(ids(r#"[class*=""]"#), "");
    assert_eq!(ids(r#"[class^="FOO" i]"#), "abdfgjkm");
}

#[test]
fn class_value_whitespace_is_normalized() {
    // The class list keeps only its tokens, so the value tested is them joined by single
    // spaces — bs4's answer. lxml, on the raw value, differs only where `class` has
    // leading/trailing whitespace (f, g, i) or a tab (k); noted per line.
    assert_eq!(ids(r#"[class="foo"]"#), "fgm"); // lxml: m
    assert_eq!(ids(r#"[class=""]"#), "hi"); // lxml: h
    assert_eq!(ids(r#"[class="foo bar"]"#), "ak"); // lxml: a
    assert_eq!(ids(r#"[class^="foo"]"#), "abfgjkm"); // lxml: abgjkm
    assert_eq!(ids(r#"[class$="foo"]"#), "cefgm"); // lxml: cefm
    assert_eq!(ids(r#"[class*="o b"]"#), "ak"); // lxml: a
    assert_eq!(ids(r#"[class|="foo"]"#), "fgjm"); // lxml: jm
    assert_eq!(ids(r#"[class|=""]"#), "hin"); // lxml: hn
}

#[test]
fn empty_substring_patterns_match_nothing() {
    let html = r#"<a id="x" href="/p" title=""></a>"#;
    let ids = |css: &str| {
        HtmlDoc::parse(html)
            .unwrap()
            .dom()
            .root()
            .select_css(css)
            .unwrap()
            .count()
    };
    assert_eq!(ids(r#"[href^=""]"#), 0);
    assert_eq!(ids(r#"[href$=""]"#), 0);
    assert_eq!(ids(r#"[href*=""]"#), 0);
    assert_eq!(ids(r#"[href~=""]"#), 0);
    assert_eq!(ids(r#"[title=""]"#), 1);
    assert_eq!(ids(r#"[title|=""]"#), 1);
}

#[test]
fn spanning_patterns_test_the_joined_value() {
    // A pattern with whitespace can only match across classes joined by single spaces.
    assert_eq!(ids(r#"[class^="foo b"]"#), "ak");
    assert_eq!(ids(r#"[class$="o bar"]"#), "ak");
    assert_eq!(ids(r#"[class$="r foo"]"#), "e");
    assert_eq!(ids(r#"[class*="foo bar"]"#), "ak");
    assert_eq!(ids(r#"[class|="foo bar"]"#), "ak");
    assert_eq!(ids(r#"[class="FOO BAR" i]"#), "ak");
    assert_eq!(ids(r#"[class*="O B" i]"#), "ak");
    // Never in a joined value: a doubled, leading or trailing space, or a tab.
    assert_eq!(ids(r#"[class="foo  bar"]"#), "");
    assert_eq!(ids(r#"[class=" foo"]"#), "");
    assert_eq!(ids(r#"[class="foo bar "]"#), "");
    assert_eq!(ids("[class=\"foo\tbar\"]"), "");
    assert_eq!(ids(r#"[class=" "]"#), "");
}

#[test]
fn class_word_match_is_parsed_as_a_class_selector() {
    use crate::css::{CompoundSelector, patterns::CssPattern};
    let parse = |css| {
        let mut chars = crate::css::CssChars::new(css);
        CompoundSelector::from_chars(&mut chars).unwrap().unwrap()
    };
    let word = parse(r#"div[class~="foo"]"#);
    assert_eq!((word.classes.len(), word.class_attributes.len()), (1, 0));
    // These keep the attribute path: no plain class selector means the same.
    for css in [
        r#"[class~="foo" i]"#,
        r#"[class~=""]"#,
        r#"[class~="foo bar"]"#,
        r#"[class~="a&amp;b"]"#,
    ] {
        let sel = parse(css);
        assert_eq!(
            (sel.classes.len(), sel.class_attributes.len()),
            (0, 1),
            "{css}"
        );
    }
    // And the rewrite still selects exactly `.foo`, also under `:not`.
    assert_eq!(ids(r#":not([class~="foo"])"#), ids(":not(.foo)"));
}
