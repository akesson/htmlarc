//! Extended (custom/unknown) tag selector matching (ADR 0002 §4, PR 4).

use super::helpers::select;
use crate::html::HtmlDoc;
use crate::prelude::*;

#[test]
fn selects_custom_element_by_tag() {
    let html = r#"<div><my-widget>a</my-widget><span>b</span><my-widget>c</my-widget></div>"#;
    assert_eq!(select(html, "my-widget"), ["my-widget", "my-widget"]);
}

#[test]
fn absent_custom_element_selects_nothing() {
    // A custom name absent from the document never matches (resolves to `Absent`).
    let html = r#"<div><my-widget>a</my-widget></div>"#;
    assert_eq!(select(html, "other-widget"), Vec::<String>::new());
    // And when the document holds no custom elements at all.
    assert_eq!(
        select("<div><p>x</p></div>", "no-such"),
        Vec::<String>::new()
    );
}

#[test]
fn custom_element_with_class_and_attribute() {
    let html = r#"<div><my-card class="hi" data-id="7">a</my-card><my-card>b</my-card></div>"#;
    assert_eq!(select(html, "my-card.hi"), ["my-card.hi"]);
    assert_eq!(select(html, "my-card[data-id]"), ["my-card.hi"]);
    assert_eq!(select(html, r#"my-card[data-id="7"]"#), ["my-card.hi"]);
}

#[test]
fn not_custom_element() {
    let html = r#"<div><my-x>a</my-x><my-y>b</my-y></div>"#;
    // Of the div's two custom children, `:not(my-x)` keeps only `my-y` (a vocab-byte compare
    // negated — correct through `:not`).
    assert_eq!(select(html, "div :not(my-x)"), ["my-y"]);
}

#[test]
fn standard_and_custom_tag_selectors_do_not_cross_match() {
    let html = r#"<div><span>s</span><my-span>c</my-span></div>"#;
    // The standard `span` selector matches only the real `<span>`; the custom `my-span`
    // selector matches only the custom element. Normalization keeps the two spaces disjoint.
    assert_eq!(select(html, "span"), ["span"]);
    assert_eq!(select(html, "my-span"), ["my-span"]);
}

#[test]
fn overflow_custom_element_is_selectable() {
    use std::fmt::Write;
    let mut html = String::from("<div>");
    for i in 0..70u32 {
        write!(html, "<x-{i}>t</x-{i}>").unwrap();
    }
    html.push_str("</div>");
    // `x-65` is past the 63-slot vocab → an overflow tag; resolve-once must still match it via
    // the overflow side map, and a vocab tag (`x-3`) via its byte.
    assert_eq!(select(&html, "x-65"), ["x-65"]);
    assert_eq!(select(&html, "x-3"), ["x-3"]);
}

#[test]
fn matches_css_exercises_the_unresolved_string_path() {
    // `matches_css` matches directly (no resolve pass), so an extended selector falls back to
    // a tag-name string compare rather than a resolved byte.
    let parsed = HtmlDoc::parse(r#"<div><my-widget>a</my-widget></div>"#).unwrap();
    let doc = parsed.dom();
    let widget = doc.root().select_css("my-widget").unwrap().next().unwrap();
    assert!(widget.matches_css("my-widget").unwrap());
    assert!(!widget.matches_css("other-widget").unwrap());
    assert!(!widget.matches_css("div").unwrap());
}

// --- foreign content selectors (ADR 0002 §5, PR 5) ---

#[test]
fn svg_and_its_children_are_selectable() {
    // `svg` is a standard enum tag; its children (`path`) are extended elements. Both are
    // now stored and queryable (they used to be dropped). `tag_id_class` reports the stored
    // lowercase name.
    let html = r#"<div><svg><path d="M0 0"></path><path d="M1 1"></path></svg></div>"#;
    assert_eq!(select(html, "svg"), ["svg"]);
    assert_eq!(select(html, "path"), ["path", "path"]);
}

#[test]
fn camelcase_svg_selector_is_case_insensitive() {
    // A type selector is ASCII-case-insensitive, and stored names are lowercase, so the
    // canonical `clipPath` (and a shouty `CLIPPATH`) resolve to the lowercased symbol.
    let html = r#"<svg><clipPath id="c"><path></path></clipPath><clipPath></clipPath></svg>"#;
    assert_eq!(select(html, "clipPath"), ["clippath#c", "clippath"]);
    assert_eq!(select(html, "CLIPPATH"), ["clippath#c", "clippath"]);
    assert_eq!(select(html, "clippath"), ["clippath#c", "clippath"]);
    // An absent camelCase name still resolves cleanly to nothing.
    assert_eq!(select(html, "feGaussianBlur"), Vec::<String>::new());
}
