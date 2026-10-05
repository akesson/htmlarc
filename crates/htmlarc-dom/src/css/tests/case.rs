//! Case rules for attribute values: the HTML standard's list, the `i`/`s` flags, and the
//! `[text]` pseudo-attribute.

use crate::css::tests::helpers::select;

#[test]
fn attribute_values_follow_the_html_case_list() {
    let html = r#"<a id="up" href="HTTP://a.example/">a</a><a id="low" href="http://b.example/">b</a>
        <script id="s" type="Text/JavaScript" language="JavaScript"></script>
        <input id="i" accept="IMAGE/PNG"><div id="d" data-mode="Dark"></div>"#;
    // `href` is not on the list: case-sensitive unless flagged.
    assert_eq!(select(html, "a[href^='http://']"), ["a#low"]);
    assert_eq!(select(html, "a[href^='http://' i]"), ["a#up", "a#low"]);
    // `type` (an `HtmlAttr`) and `language`/`accept` (stored as extended) are on it.
    assert_eq!(select(html, "[type='text/javascript']"), ["script#s"]);
    assert_eq!(select(html, "[language='javascript']"), ["script#s"]);
    assert_eq!(select(html, "[accept='image/png']"), ["input#i"]);
    assert!(select(html, "[type='text/javascript' s]").is_empty());
    // Other extended names stay case-sensitive.
    assert!(select(html, "[data-mode='dark']").is_empty());
    assert_eq!(select(html, "[data-mode='dark' i]"), ["div#d"]);
}

#[test]
fn text_pseudo_attribute_folds_unicode_case() {
    let html = r#"<p id="p">un été chaud</p>"#;
    assert_eq!(select(html, r#"p[text*="ÉTÉ" i]"#), ["p#p"]);
    assert!(select(html, r#"p[text*="ÉTÉ"]"#).is_empty());
}
