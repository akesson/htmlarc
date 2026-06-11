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
