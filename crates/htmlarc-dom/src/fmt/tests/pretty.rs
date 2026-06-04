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
    assert_snapshot!(format(FORMATTED_HTML, HtmlFormat::Pretty), @r###"
    <!DOCTYPE html>
     
    <html>
    	<head>
     
    		<title>
    hello
    		</title>
     
    	</head>
    	<body>
    		<div class="cls" data-mw="whatever" lang="en">
    			<p>
    				<span id="hlo">
    hello
    				</span>
    			</p>
    		</div>
    	</body>
    </html>
    "###);
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
