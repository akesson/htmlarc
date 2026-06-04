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

        if false {
            let mut outpath = path.to_path_buf();
            outpath.set_extension("out.html");
            html.write_to(&outpath);
        }
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
