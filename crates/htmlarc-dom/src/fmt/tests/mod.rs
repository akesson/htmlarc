mod pretty;

use std::fs;

use insta::{assert_snapshot, glob};

use crate::prelude::*;

const SIMPLE_HTML: &str =
    r#"<body><div class="cls1 cls2"><p>hello</p></div><br /><span>more</span></body>"#;

fn format(html: &str, format: HtmlFormat) -> String {
    let html = HtmlDoc::parse(html).unwrap();
    html.dom.to_html(format)
}

#[test]
fn simple_html_raw() {
    assert_eq!(format(SIMPLE_HTML, HtmlFormat::Raw), SIMPLE_HTML);
}

const HTML_INLINE: &str =
    r#"<body><div><b>hello <i>there</i></b> <strong>friend</strong></div><p>ship</p></body>"#;

#[test]
fn inline_html_raw() {
    assert_eq!(format(HTML_INLINE, HtmlFormat::Raw), HTML_INLINE);
}

#[test]
fn pretty_format() {
    glob!("html/*.html", |path| {
        let html = fs::read_to_string(path).unwrap();
        assert_snapshot!(format(&html, HtmlFormat::Pretty));
    });
}

/// Written HTML is UTF-8, so a stored legacy `<meta>` label is written as `utf-8`; queries still
/// see the page's own label.
#[test]
fn meta_charset_labels_are_written_as_utf8() {
    const HTML: &str = concat!(
        r#"<html><head><meta charset="windows-1251">"#,
        r#"<meta http-equiv="Content-Type" content="text/html; charset=windows-1251">"#,
        r#"<meta name="description" content="charset=gbk"></head><body><p>Привет</p></body></html>"#,
    );
    for fmt in [HtmlFormat::Raw, HtmlFormat::Pretty] {
        let written = format(HTML, fmt);
        assert!(written.contains(r#"<meta charset="utf-8""#), "{written}");
        assert!(
            written.contains(r#"content="text/html; charset=utf-8""#),
            "{written}"
        );
        assert!(written.contains(r#"content="charset=gbk""#), "{written}");
        assert!(!written.contains("1251"), "{written}");
    }
    let dom = HtmlDoc::parse(HTML).unwrap().dom();
    let meta = dom.root().select_css("meta").unwrap().next().unwrap();
    assert_eq!(
        meta.get_attribute("charset").as_deref(),
        Some("windows-1251")
    );
}
