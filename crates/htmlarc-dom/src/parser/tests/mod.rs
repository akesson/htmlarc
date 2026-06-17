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
fn mismatched_custom_end_tag_does_not_close_a_different_element() {
    // Two distinct custom elements share the `extended` kind, but full identity keeps them
    // apart: `</b-b>` cannot close `<a-a>`. The mismatched end tag matches no open element,
    // so it is ignored (ADR 0003 round 2) and `<a-a>` closes at EOF — it is never closed by
    // a differently-named end tag.
    roundtrip_to("<a-a></b-b>", "<a-a></a-a>");
    // A standard end tag cannot close a custom element, nor vice versa — each is ignored.
    roundtrip_to("<x-y></div>", "<x-y></x-y>");
    roundtrip_to("<div></x-y>", "<div></div>");
    // Proof the orphan close was ignored, not treated as closing `<a-a>`: trailing content
    // still lands inside `<a-a>` rather than escaping it.
    roundtrip_to("<a-a></b-b>more</a-a>", "<a-a>more</a-a>");
    // Matching identity still closes cleanly.
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

// --- foreign content (ADR 0002 §5, PR 5) ---

#[test]
fn svg_subtree_round_trips_with_case_restored() {
    // svg/math subtrees are now stored as ordinary (extended) elements; the WHATWG SVG name
    // tables restore the canonical case at the formatter, so a camelCase document is its own
    // fixed point.
    roundtrip(
        r#"<svg viewBox="0 0 10 10"><clipPath id="c"><path d="M0 0"></path></clipPath></svg>"#,
    );
    // The lowercased spellings html5gum hands us still render canonical.
    roundtrip_to(
        r#"<svg viewbox="0 0 10 10"><clippath id="c"><path d="M0 0"></path></clippath></svg>"#,
        r#"<svg viewBox="0 0 10 10"><clipPath id="c"><path d="M0 0"></path></clipPath></svg>"#,
    );
    // A standard attribute on an svg element (e.g. `id`, `class`) is unaffected by the table.
    roundtrip(
        r#"<svg class="icon" id="i"><feGaussianBlur stdDeviation="2"></feGaussianBlur></svg>"#,
    );
}

#[test]
fn mathml_subtree_round_trips() {
    roundtrip("<math><mrow><mi>x</mi><mo>+</mo><mn>1</mn></mrow></math>");
    // MathML's lone case-adjusted attribute.
    roundtrip_to(
        r#"<math><mi definitionurl="u">x</mi></math>"#,
        r#"<math><mi definitionURL="u">x</mi></math>"#,
    );
}

#[test]
fn self_closing_foreign_child_renders_childless() {
    // A self-closing `<path/>` is popped immediately and renders `<path></path>` (no void or
    // self-closing semantics for extended elements — ADR 0002 §5).
    roundtrip_to(
        r#"<svg><path d="M0 0"/></svg>"#,
        r#"<svg><path d="M0 0"></path></svg>"#,
    );
    // A self-closing foreign root is empty, not the parent of what follows.
    roundtrip_to("<svg/><div></div>", "<svg></svg><div></div>");
}

#[test]
fn foreign_object_holds_html_children() {
    roundtrip(
        r#"<svg viewBox="0 0 1 1"><foreignObject><div class="x">hi</div></foreignObject></svg>"#,
    );
}

#[test]
fn unclosed_foreign_children_recover_to_matching_ancestor() {
    // The common real-world SVG-icon pattern: a `<path>` left open (no `/`, no `</path>`).
    // A real foreign-content tree builder implicitly closes it at the ancestor's end tag;
    // tolerant recovery (ADR 0002 §5) pops the unclosed child instead of failing the whole
    // document, which it used to do (and which the skip machinery masked by dropping svg).
    roundtrip_to(
        r#"<svg><path d="M0 0"></svg>"#,
        r#"<svg><path d="M0 0"></path></svg>"#,
    );
    // Several unclosed children, and an unclosed child nested in an unclosed `<symbol>`.
    roundtrip_to(
        r#"<svg><symbol><path><circle></symbol><path></svg>"#,
        r#"<svg><symbol><path><circle></circle></path></symbol><path></path></svg>"#,
    );
    // Recovery is not svg-specific — it is general end-tag error recovery: `</div>` closes a
    // still-open `<span>`.
    roundtrip_to("<div><span></div>", "<div><span></span></div>");
}

#[test]
fn unmatched_end_tag_is_ignored() {
    // An end tag matching no open element is ignored, not fatal (ADR 0003 round 2). HTML5's
    // tree construction drops it and keeps building; failing the whole document over one
    // orphan close discards an otherwise-extractable page. This supersedes the earlier stance
    // (ADR 0002 PR 5), which kept such tags a parse error to surface structural corruption —
    // the wrong trade for an extraction archive, where it cost ~20% of the corpus.
    // `</g>` matches nothing and is dropped; `</svg>` then stack-walks the still-open `<path>`
    // and `<svg>` (the deeper-match recovery from ADR 0002 §5).
    roundtrip_to("<svg><path></g></svg>", "<svg><path></path></svg>");
    // `</section>` matches nothing, so it is dropped; the `<div>` auto-closes at EOF.
    roundtrip_to("<div></section>", "<div></div>");
}

#[test]
fn cdata_in_foreign_content_is_text() {
    // Inside foreign content `<![CDATA[…]]>` is character data; its markup-significant bytes
    // are re-encoded on output. Outside foreign content it stays a bogus comment (unchanged).
    roundtrip_to(
        "<svg><desc><![CDATA[a < b & c]]></desc></svg>",
        "<svg><desc>a &lt; b &amp; c</desc></svg>",
    );
    roundtrip_to("<div><![CDATA[x]]></div>", "<div><!--[CDATA[x]]--></div>");
}

#[test]
fn raw_text_is_suppressed_inside_foreign_content() {
    // `<title>`/`<style>`/`<script>` are RCDATA/RAWTEXT only in the HTML namespace. Inside
    // svg they are ordinary elements: a `<title>` parses child markup, and a `<style>` body
    // is treated as text (decoded on ingest, entity-encoded on output).
    roundtrip("<svg><title><b>x</b></title></svg>");
    roundtrip_to(
        "<svg><style>x&y</style></svg>",
        "<svg><style>x&amp;y</style></svg>",
    );
    // The same elements at the top level keep their HTML-namespace raw-text behaviour.
    roundtrip_to(
        "<title><b>x</b></title>",
        "<title>&lt;b&gt;x&lt;/b&gt;</title>",
    );
    roundtrip("<style>x&y</style>");
}

#[test]
fn foreign_content_pretty_formats() {
    // The pretty formatter resolves the same case tables and never leaks the `extended`
    // marker for svg children.
    let doc = r#"<svg viewBox="0 0 1 1"><clipPath><path d="M0 0"></path></clipPath></svg>"#;
    let pretty = HtmlDoc::parse(doc).unwrap().to_html(HtmlFormat::Pretty);
    assert!(pretty.contains("viewBox=\"0 0 1 1\""), "pretty: {pretty}");
    assert!(
        pretty.contains("<clipPath>") && pretty.contains("</clipPath>"),
        "pretty: {pretty}"
    );
    assert!(!pretty.contains("extended"), "pretty: {pretty}");
}

// --- parser error recovery (ADR 0003) ---

#[test]
fn self_closing_slash_on_html_element_is_ignored() {
    // `<div/>` is XML self-closing syntax on a non-void HTML element. HTML5 ignores the slash
    // and keeps the element open, so the later `</div>` matches. htmlarc used to honor the
    // slash, self-close the `<div>`, and then orphan the `</div>` — discarding the *whole*
    // document. This was the dominant (~97.9 %) structural-failure bucket behind the 24 %
    // document-loss rate (ADR 0003).
    roundtrip_to(r#"<div id="x"/></div>"#, r#"<div id="x"></div>"#);
    // The element stays open and absorbs the following content up to its real end tag.
    roundtrip_to("<div/>text</div>", "<div>text</div>");
    roundtrip_to("<p/>hi</p>", "<p>hi</p>");
    // No matching end tag: it auto-closes at EOF, still childless — same as the bare tag.
    roundtrip_to("<section/>", "<section></section>");
}

#[test]
fn self_closing_foreign_siblings_stay_siblings() {
    // The fix above must stay foreign-aware. An SVG icon sprite is a run of self-closing
    // siblings; each `<path/>` must pop as a sibling, not nest inside the previous one. svg
    // children are stored as `extended` (ADR 0002 §5), indistinguishable from a non-foreign
    // custom element by tag alone, so the self-closing flag is honored only while inside a
    // foreign subtree (tracked by depth). A naive tag-only gate would silently nest these.
    roundtrip_to(
        r#"<svg><path d="M0"/><path d="M1"/></svg>"#,
        r#"<svg><path d="M0"></path><path d="M1"></path></svg>"#,
    );
    // Nested foreign groups: depth must rise and fall so deeper self-closing children still
    // pop, and content after the subtree returns to HTML rules.
    roundtrip_to(
        r#"<svg><g><path/><path/></g><rect/></svg><div/>x"#,
        r#"<svg><g><path></path><path></path></g><rect></rect></svg><div>x</div>"#,
    );
}

#[test]
fn stray_void_end_tags_are_ignored() {
    // Void elements have no end tag; an explicit `</source>` is a parse error HTML5 ignores,
    // not a document-killer. `<audio>`/`<video>`/`<picture>` pages commonly write
    // `<source>…</source>` pairs. `source` is void (WHATWG), so each pops at its own start tag
    // and the stray close is dropped, leaving childless siblings (ADR 0003).
    roundtrip_to(
        r#"<audio><source src="a.ogg"></source><source src="a.mp3"></source></audio>"#,
        r#"<audio><source src="a.ogg"><source src="a.mp3"></audio>"#,
    );
    // The close can appear with no matching open element at all — still ignored, the
    // surrounding document is preserved rather than discarded.
    roundtrip_to("<p>text</source></p>", "<p>text</p>");
}

#[test]
fn unmatched_html_end_tags_are_ignored() {
    // The dominant remaining failure bucket (ADR 0003 round 2): orphan and over-eager end
    // tags from messy scraped HTML. Each closes no open element, so HTML5 drops it; htmlarc
    // used to discard the whole document. An orphan inline `</span>` inside a `<p>` is gone:
    roundtrip_to("<div><p>hi</span></p></div>", "<div><p>hi</p></div>");
    // Over-closing — extra `</div>`s past the matching one — is ignored.
    roundtrip_to("<div>x</div></div></div>", "<div>x</div>");
    // A stray block close in the middle of content does not drop the surrounding element.
    roundtrip_to("<main><p>x</p></aside></main>", "<main><p>x</p></main>");
}

#[test]
fn stray_end_tag_on_empty_stack_is_ignored() {
    // An end tag with nothing open at all (the stack is empty) is dropped rather than failing
    // the parse (ADR 0003 round 2; was the "Closing a tag, but none open" error).
    roundtrip_to("</div><p>hi</p>", "<p>hi</p>");
    roundtrip_to("</a></b></c><p>x</p>", "<p>x</p>");
}

#[test]
fn stray_foreign_end_tag_does_not_corrupt_following_parse() {
    // A stray `</svg>`/`</math>` is now ignored instead of failing the document; the
    // emitter/driver foreign-depth counters saturate at zero (they never underflow), so HTML
    // parsing continues normally afterwards.
    roundtrip_to("</svg><div>x</div>", "<div>x</div>");
    roundtrip_to("<div></svg>x</div>", "<div>x</div>");
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
    // The first 256 levels are kept inline (no heap); deeper nesting spills the parse stack
    // to the heap and still parses — it used to hit a hard 256 cap that dropped the whole
    // document. Just past the inline boundary must therefore parse, not fail.
    let inline = format!("{}{}", "<div>".repeat(256), "</div>".repeat(256));
    assert!(
        HtmlDoc::parse(&inline).is_ok(),
        "256-deep (inline) nesting must parse"
    );
    let spilled = format!("{}{}", "<div>".repeat(257), "</div>".repeat(257));
    assert!(
        HtmlDoc::parse(&spilled).is_ok(),
        "257-deep nesting must spill to the heap and parse"
    );

    // The 8,192 sanity ceiling still trips: 8,192 levels parse, the 8,193rd is poisoned.
    let cap = format!("{}{}", "<div>".repeat(8192), "</div>".repeat(8192));
    assert!(HtmlDoc::parse(&cap).is_ok(), "8192-deep nesting must parse");
    let too_deep = format!("{}{}", "<div>".repeat(8193), "</div>".repeat(8193));
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
