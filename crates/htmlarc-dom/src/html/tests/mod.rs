use std::fs;

use insta::{assert_snapshot, glob};

use crate::{iters::ElementIter, prelude::*};

#[test]
fn roundtrip() {
    glob!("html/*.html", |path| {
        let html = fs::read_to_string(path).unwrap();
        let doc = HtmlDoc::parse(&html).unwrap();

        assert_snapshot!(doc.to_html(HtmlFormat::Raw));
    });
}

#[test]
fn pretty_format() {
    glob!("html/*.html", |path| {
        let html = fs::read_to_string(path).unwrap();
        let doc = HtmlDoc::parse(&html).unwrap();

        assert_snapshot!(doc.to_html(HtmlFormat::Pretty));
    });
}

#[test]
fn remove_formatting() {
    glob!("html/*.html", |path| {
        let html = fs::read_to_string(path).unwrap();
        let doc = HtmlDoc::parse(&html).unwrap().dom_ref_cell();

        doc.with_mut(|dom| dom.remove_formatting());
        assert_snapshot!(doc.to_html(HtmlFormat::Raw));
    });
}

// TODO: investigate why this test is failing
#[ignore = "to investigate"]
#[test]
fn repackage_html() {
    const HTML: &str = r###"<head></head><body><div class="bg-blue-500 text-xl" border="12px"><b>text</b></div><article class="items-center uppercase p-4"><h1 class="m-2">text1</h1><p>hi</p><span>hello</span><i>link</i></article><section title="empty" style="border: 1px solid red;"></section></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();

    let mut iter = html.root().forwards();
    assert_next(&mut iter, HtmlTag::head);
    assert_next(&mut iter, HtmlTag::body);
    assert_next(&mut iter, HtmlTag::div);
    assert_next(&mut iter, HtmlTag::b);
    assert_next(&mut iter, HtmlTag::sys_text);
    assert_next(&mut iter, HtmlTag::article);
    iter.next().unwrap().unwrap_element(); // h1
    iter.next().unwrap(); // text1

    iter.next().unwrap().unwrap_element(); // p
    iter.next().unwrap(); // added space
    iter.next().unwrap(); // hi
    iter.next().unwrap(); // added space

    iter.next().unwrap().unwrap_element(); // span
    iter.next().unwrap(); // hello

    iter.next().unwrap().unwrap_element(); // i

    let new_doc = html.repackage();

    assert_eq!(
        r###"<head></head><body><div class="bg-blue-500 text-xl" border="12px"><b>text</b></div><article class="items-center uppercase p-4">text1 hi hellolink</article><section title="empty" style="border: 1px solid red;"></section></body>"###,
        new_doc.to_html(HtmlFormat::Raw)
    );

    assert_snapshot!("repackaged_doc_nodes", format!("{:?}", new_doc.nodes));
}

#[track_caller]
fn assert_next(iter: &mut ElementIter<DomRefCell>, tag: HtmlTag) {
    assert_eq!(iter.next().map(|n| n.tag()), Some(tag));
}

#[test]
fn repackage_round_trip() {
    glob!("html/*.html", |path| {
        let html_str = fs::read_to_string(path).unwrap();
        let html = HtmlDoc::parse(&html_str).unwrap().dom_ref_cell();
        let ref_str = html.to_html(HtmlFormat::Raw);

        let new_str = html.repackage().to_html(HtmlFormat::Raw);
        assert_eq!(ref_str, new_str);
    });
}

#[test]
fn logging() {
    const HTML: &str =
        r###"<body><section title="empty" style="border: 1px solid red;"></section></body>"###;

    let html = HtmlDoc::parse(HTML).unwrap().dom_ref_cell();
    let mut iter = html.root().forwards();
    iter.next();
    iter.next().unwrap().log(|| "empty section");

    assert_snapshot!(html.to_html(HtmlFormat::Pretty), @r###"

    <body>
    <!-- [htmlarc_dom::html::tests::logging]
    empty section -->
    	<section title="empty" style="border: 1px solid red;"></section>
    </body>
    "###);
}

#[test]
fn select_css() {
    fn print_elements<'a, I>(elements: I) -> String
    where
        I: Iterator<Item = HtmlElement<'a, DomOwn>>,
    {
        elements
            .map(|el| format!("{} - {}", el.index(), el.tag()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    glob!("html/*.html", |path| {
        let html = fs::read_to_string(path).unwrap();
        let dom = HtmlDoc::parse(&html).unwrap().dom();
        let root = dom.root();

        let selector = r#"[class^="mw-ui"]"#;
        let selected = root.select_css(selector).unwrap();

        assert_snapshot!(print_elements(selected));
    });
}

#[test]
fn select_css_surfaces_positional_parse_error() {
    use crate::css::Diagnostic;

    let doc = HtmlDoc::parse("<body><a></a></body>").unwrap().dom();
    // A malformed selector returns the rich `css::ParseError` directly, not a
    // flattened string — so `diagnosis()` can still render the offending input
    // with an ANSI underline at the error position.
    let err = match doc.root().select_css("a[") {
        Ok(_) => panic!("expected a parse error for malformed selector"),
        Err(e) => e,
    };
    let diag = err.diagnosis("a[");
    assert!(
        diag.contains('\u{1b}'),
        "expected an ANSI-underlined positional diagnostic, got: {diag:?}"
    );

    // The convenience boolean matcher threads the same error type, and returns a
    // bool for a valid selector.
    assert!(doc.root().matches_css("a[").is_err());
    let a = doc
        .root()
        .select_css("a")
        .unwrap()
        .next()
        .expect("an <a> element");
    assert!(a.matches_css("a").unwrap());
    assert!(!a.matches_css("p").unwrap());
}
