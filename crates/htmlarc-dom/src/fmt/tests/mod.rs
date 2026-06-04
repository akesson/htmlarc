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
