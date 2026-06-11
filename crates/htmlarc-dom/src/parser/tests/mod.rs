use crate::prelude::*;
use insta::{assert_snapshot, glob};
use std::fs;

#[test]
fn full_doc_test_dom() {
    glob!("html/*.html", |path| {
        println!("path: {:?}", path);
        let html_string = fs::read_to_string(path).unwrap();
        let _html = match HtmlDoc::test_parse(html_string.as_str()) {
            Err(e) => {
                panic!("{e}");
            }
            Ok(val) => val,
        };
    });
}

#[test]
fn full_doc_with_dom() {
    glob!("html/*.html", |path| {
        println!("path: {:?}", path);
        let html_string = fs::read_to_string(path).unwrap();
        let html = match HtmlDoc::parse(html_string.as_str()) {
            Err(e) => {
                panic!("{e}");
            }
            Ok(val) => val,
        };
        assert_snapshot!(html.to_html(HtmlFormat::Raw));
    });
}

#[test]
fn simple_doc() {
    let html = HtmlDoc::parse(DOC1).unwrap().to_html(HtmlFormat::Raw);
    assert_eq!(html.trim(), DOC1.trim());
}

const DOC1: &str = r##"
<!DOCTYPE html>
<html class="client-nojs mf-expand-sections-clientpref-0 mf-font-size-clientpref-small mw-mf-amc-clientpref-0" lang="en" dir="ltr">
<head>
    <meta name="referrer" content="origin">
</head>
<body class="mediawiki ltr">bodytext</body>
</html>
"##;

#[test]
fn doc_with_data_attributes() {
    roundtrip("<div data-foo=\"bar\"></div>");
}

// --- unified attribute store (ADR 0002 §3, PR 3) ---

#[test]
fn unknown_attribute_name_round_trips() {
    // Unknown names are no longer a parse error — they are kept as extended attributes.
    roundtrip("<div wonky=\"yes\"></div>");
    roundtrip("<div data-x-data-y=\"1\"></div>");
    // Bare (valueless) standard + unknown attributes on a void element.
    roundtrip("<input disabled custom />");
}

#[test]
fn attribute_source_order_is_preserved() {
    // Standard, data-*, and unknown attributes now share one run rendered in source order
    // (the old store rendered class, then data-*, then standard).
    roundtrip(r#"<div data-a="1" href="/h" data-b="2" lang="en"></div>"#);
    roundtrip(r#"<a href="/x" data-mw="i" title="t"></a>"#);
}

#[test]
fn duplicate_attribute_names_both_kept() {
    // html5gum streams duplicates; only distinct (name, value) pairs dedup. Matches today's
    // behaviour (the WHATWG first-wins rule is an open question, not bundled here).
    roundtrip(r#"<a id="a" id="b"></a>"#);
    // Identical (name, value) pairs collapse to one.
    roundtrip_to(r#"<a id="a" id="a"></a>"#, r#"<a id="a"></a>"#);
}

#[test]
fn class_list() {
    roundtrip(r##"<html class="mf"><body class="mediawiki"><div class=""></div></body></html>"##);
}

#[test]
fn tag_closing() {
    roundtrip("<link>");
    roundtrip("<meta>");
    roundtrip("<span></span>");
    roundtrip("<br />");
    roundtrip("<img />");
    roundtrip("<input />");
    roundtrip("<!DOCTYPE html>");
    roundtrip_to("<svg><some elem></svg>", "");
}

#[test]
fn body_p_section() {
    roundtrip("<body><p><section></section></p></body>")
}

// --- extended tags (ADR 0002 §4, PR 4) ---

#[test]
fn custom_element_round_trips() {
    // Unknown tag names are no longer a parse error — they parse as extended (custom)
    // elements and render with their real name (never the `extended` marker).
    roundtrip(r#"<my-widget class="x"><span>hi</span></my-widget>"#);
    roundtrip("<x-y></x-y>");
    roundtrip(r#"<my-card data-id="7" title="t"><p>body</p></my-card>"#);
    // A self-closing custom element has no void semantics: it renders an explicit close tag.
    roundtrip_to("<x-y/>", "<x-y></x-y>");
    roundtrip_to("<x-y />", "<x-y></x-y>");
}

#[test]
fn nested_distinct_custom_elements_round_trip() {
    roundtrip("<a-a><b-b><c-c></c-c></b-b></a-a>");
}

#[test]
fn custom_element_with_standard_and_extended_attributes() {
    roundtrip(r#"<my-el id="a" data-x="1" href="/h" wonky="y"></my-el>"#);
}

#[test]
fn demoted_mediawiki_tags_round_trip_as_extended() {
    // `hnan` and `figure-inline` were demoted from the `HtmlTag` enum (ADR 0002 §4); they now
    // parse as ordinary extended tags, indistinguishable from any other custom element.
    roundtrip("<hnan></hnan>");
    roundtrip("<figure-inline></figure-inline>");
}

#[test]
fn reserved_spellings_parse_as_custom_elements() {
    // `extended`, `text`, and `comment` would alias onto a normalization/system variant via
    // strum's case-insensitive `FromStr`; they must round-trip as custom elements instead —
    // in particular `<extended>` is a custom element, never the `extended` marker spelling.
    roundtrip("<extended></extended>");
    roundtrip("<text></text>");
    roundtrip("<comment></comment>");
}

#[test]
fn mismatched_custom_end_tag_is_a_parse_error() {
    // Two distinct custom elements share the `extended` kind, but full identity keeps them
    // apart: `</b-b>` cannot close `<a-a>`. The error names the offending tags.
    let err = parse_error("<a-a></b-b>");
    assert!(
        err.contains("a-a") && err.contains("b-b"),
        "error should name both custom tags: {err}"
    );
    // A standard end tag cannot close a custom element, nor vice versa.
    assert!(parse_error("<x-y></div>").contains("x-y"));
    assert!(parse_error("<div></x-y>").contains("x-y"));
    // Matching identity closes cleanly.
    roundtrip("<a-a></a-a>");
}

#[test]
fn standard_auto_close_fires_inside_a_custom_element() {
    // `<li>`→`<li>` implied end tags still fire under a custom ancestor (an extended element
    // itself never auto-closes), and the custom element opens/closes by identity.
    roundtrip_to(
        "<x-y><ul><li>a<li>b</li></ul></x-y>",
        "<x-y><ul><li>a</li><li>b</li></ul></x-y>",
    );
}

#[test]
fn many_distinct_custom_elements_overflow_vocab_and_round_trip() {
    use std::fmt::Write;
    // 70 distinct custom-element names exceed the 63-slot vocab; the surplus spill to the
    // overflow side map and must still round-trip byte-exact (ADR 0002 §4).
    let mut html = String::from("<div>");
    for i in 0..70u32 {
        write!(html, "<x-{i}>t</x-{i}>").unwrap();
    }
    html.push_str("</div>");
    roundtrip(&html);
}

#[test]
fn extended_marker_never_leaks_into_output() {
    // Both formatters must render the real custom-element name, never the `extended` marker
    // spelling that `nodes.tag()` normalizes to.
    let doc = "<div><my-widget data-x=\"1\"><span>hi</span></my-widget></div>";
    let parsed = HtmlDoc::parse(doc).unwrap();
    let raw = parsed.to_html(HtmlFormat::Raw);
    let pretty = parsed.to_html(HtmlFormat::Pretty);
    assert!(raw.contains("<my-widget") && raw.contains("</my-widget>"));
    assert!(pretty.contains("<my-widget") && pretty.contains("</my-widget>"));
    assert!(
        !raw.contains("extended") && !pretty.contains("extended"),
        "the `extended` marker must never appear:\n raw: {raw}\n pretty: {pretty}"
    );
}

// --- per-document overflow guardrails (ADR 0002, PR 1) ---
//
// Each pathological document below used to either silently corrupt its stores (a u16
// index wrapping onto an earlier entry, or a list spliced into a cycle) or panic the
// whole import (the fixed-capacity parse stacks). They must now fail as ordinary,
// per-document parse errors so an import can skip the doc and continue.

/// Parse `html`, asserting it fails, and return the (capacity) error message. Avoids
/// `expect_err`, which would require `HtmlDoc: Debug`.
#[track_caller]
fn parse_overflow(html: &str) -> String {
    match HtmlDoc::parse(html) {
        Ok(_) => panic!("expected a per-document overflow error, but parse succeeded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn attribute_value_overflow_is_a_per_document_error() {
    use std::fmt::Write;
    // Every <a> carries a distinct id value, so the per-document string heap (65,535
    // entries) overflows without ever filling a single attribute list.
    let mut html = String::new();
    for i in 0..66_000u32 {
        write!(html, "<a id=\"v{i}\"></a>").unwrap();
    }
    assert!(parse_overflow(&html).contains("capacity"));
}

#[test]
fn class_list_overflow_is_a_per_document_error() {
    use std::fmt::Write;
    // The class-run arena holds at most 65,535 slots (values + one terminator per list).
    // 9,000 elements × 7 classes = 72,000 slots — drawn from a pool of 70 distinct names
    // so the symbol table (61,184 cap) stays far below its own ceiling and the *arena*
    // is what overflows.
    let mut html = String::new();
    for i in 0..9_000u32 {
        html.push_str("<i class=\"");
        for j in 0..7u32 {
            write!(html, "c{} ", (i * 7 + j) % 70).unwrap();
        }
        html.push_str("\"></i>");
    }
    assert!(parse_overflow(&html).contains("capacity"));
}

#[test]
fn nesting_depth_boundary() {
    // 256 levels parse; the 257th trips the depth guard (previously a hard panic).
    let ok = format!("{}{}", "<div>".repeat(256), "</div>".repeat(256));
    assert!(HtmlDoc::parse(&ok).is_ok(), "256-deep nesting must parse");

    let too_deep = format!("{}{}", "<div>".repeat(257), "</div>".repeat(257));
    assert!(parse_overflow(&too_deep).contains("capacity"));
}

/// Parse `html`, asserting it fails, and return the error message. Like
/// [`parse_overflow`] but for ordinary (non-capacity) parse errors, e.g. a tag mismatch.
#[track_caller]
fn parse_error(html: &str) -> String {
    match HtmlDoc::parse(html) {
        Ok(_) => panic!("expected a parse error, but parse succeeded"),
        Err(e) => e.to_string(),
    }
}

#[track_caller] // Will show the location of the caller in test failure messages
fn roundtrip(s: &str) {
    let html = HtmlDoc::parse(s.trim()).unwrap().to_html(HtmlFormat::Raw);
    assert_eq!(html.trim(), s.trim())
}

#[track_caller] // Will show the location of the caller in test failure messages
fn roundtrip_to(s: &str, to: &str) {
    let html = HtmlDoc::parse(s.trim()).unwrap().to_html(HtmlFormat::Raw);
    assert_eq!(html.trim(), to.trim())
}
