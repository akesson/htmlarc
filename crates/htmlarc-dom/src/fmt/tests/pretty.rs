use insta::assert_snapshot;

use super::{HTML_INLINE, SIMPLE_HTML, format};
use crate::prelude::*;

#[test]
fn simple_html() {
    assert_snapshot!(format(SIMPLE_HTML, HtmlFormat::Pretty), @r###"

	<body>
		<div class="cls1 cls2">
			<p>
	hello
			</p>
		</div>
		<br />
		<span>
	more
		</span>
	</body>
	"###);
}

#[test]
fn inline_html() {
    assert_snapshot!(format(HTML_INLINE, HtmlFormat::Pretty), @r###"

	<body>
		<div>
	<b>hello <i>there</i></b> <strong>friend</strong>
		</div>
		<p>
	ship
		</p>
	</body>
	"###);
}

const FORMATTED_HTML: &str = r#"
<!DOCTYPE html>  <html>
	<head>        <title>hello</title>  </head>
	<body>
		<div class="cls" lang="en" data-mw="whatever">
			<p>
				<span id="hlo">hello</span>
			</p>
		</div>
	</body>
</html>
"#;

#[test]
fn inline_formatted_html() {
    assert_snapshot!(format(FORMATTED_HTML, HtmlFormat::Pretty), @r#"
    <!DOCTYPE html>
     
    <html>
    	<head>
     
    		<title>
    hello
    		</title>
     
    	</head>
    	<body>
    		<div class="cls" lang="en" data-mw="whatever">
    			<p>
    				<span id="hlo">
    hello
    				</span>
    			</p>
    		</div>
    	</body>
    </html>
    "#);
}

#[test]
fn inline_test() {
    const INLINE_TEST: &str = r#"<li><sup><a>(de)</a></sup><b>f</b></li>"#;

    assert_snapshot!(format(INLINE_TEST, HtmlFormat::Pretty), @r###"

	<li>
		<sup>
			<a>
	(de)
			</a>
		</sup>
	<b>f</b>
	</li>
	"###);
}

#[test]
fn empty_elements_stay_on_one_line() {
    // An empty element — including an external `<script src>` with no content — must render
    // inline and must not acquire a spurious empty text child that would split it across
    // lines. Regression guard for the html5gum tokenizer switch.
    const HTML: &str = r#"<body><script src="a.js"></script><p></p><span>hi</span></body>"#;

    assert_snapshot!(format(HTML, HtmlFormat::Pretty), @r###"

	<body>
		<script src="a.js"></script>
		<p></p>
		<span>
	hi
		</span>
	</body>
	"###);
}

#[test]
fn script_and_style_content_is_not_entity_encoded() {
    // RAWTEXT (script/style) is stored verbatim, so pretty serialization must emit it
    // unescaped — entity-encoding would corrupt `&&`, `<`, `>` in JS/CSS. Regression guard
    // alongside RawFormat's `rawtext_depth`; the shared `push_trimmed_text` previously
    // encoded everything.
    const HTML: &str = r#"<body><script>if (a && b < c) { x = "&"; }</script><style>a::before{content:"<&>"}</style></body>"#;

    let out = format(HTML, HtmlFormat::Pretty);
    assert!(
        out.contains(r#"if (a && b < c) { x = "&"; }"#),
        "script must stay verbatim, got:\n{out}"
    );
    assert!(
        out.contains(r#"a::before{content:"<&>"}"#),
        "style must stay verbatim, got:\n{out}"
    );
    assert!(
        !out.contains("&amp;") && !out.contains("&lt;") && !out.contains("&gt;"),
        "rawtext must not be entity-encoded, got:\n{out}"
    );
}

#[test]
fn inline_test2() {
    const INLINE_TEST: &str = r#"<li><sup><a>(de)</a></sup> <b>f</b></li>"#;

    assert_snapshot!(format(INLINE_TEST, HtmlFormat::Pretty), @r###"

	<li>
		<sup>
			<a>
	(de)
			</a>
		</sup>
	 <b>f</b>
	</li>
	"###);
}
