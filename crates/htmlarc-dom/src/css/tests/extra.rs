use crate::{
    css::{parse_css, tests::helpers::select},
    dom::DomRead,
    html::HtmlDoc,
};

#[test]
fn complex_relative_selector_with_multiple_potential_match() {
    let html = r#"
    <div id="id1">
        <header>
            <p class="red"></p>
            <p>
                <span class="blue"></span>
            </p>
        </header>
    </div>
    <div id="id2">
        <section id="id3">
            <p class="red">
                <div>
                    <span class="blue"></span>
                </div>
            </p>
        </section>
    </div>
    "#;

    assert_eq!(
        select(html, ":has(p.red span.blue)"),
        ["div#id2", "section#id3"]
    );
}

#[test]
fn entity_decoded_selector_literals_match_decoded_storage() {
    // Storage is entity-decoded (`&amp;` -> `&`), and selector string literals are
    // decoded the same way, so the source-oriented `&amp;` authoring style keeps
    // matching. Closes the gap where data was decoded but selectors were not — a
    // path the existing CSS suite never exercised.
    let html = r#"<a id="x" href="/p?a=1&amp;b=2">x</a><a id="y" href="/p?c=3">y</a>"#;

    assert_eq!(select(html, r#"a[href*="&amp;"]"#), ["a#x"]); // entity-form literal -> '&'
    assert_eq!(select(html, r#"a[href*="&"]"#), ["a#x"]); //     bare-ampersand literal
    assert_eq!(select(html, r#"a[href$="c=3"]"#), ["a#y"]); //   ordinary attr matching
    assert!(select(html, r#"a[href*="&xyz"]"#).is_empty()); //   absent substring
}

#[test]
fn entity_selectors_match_decoded_storage_across_all_paths() {
    // The decode is hoisted into QuotedString, so EVERY match path is covered by one
    // decode: regular attribute, data-attribute, and text content. The data-attribute
    // path in particular was silently unmatched before the hoist.
    let html = r#"<a id="a" href="/p?x=1&amp;y=2" data-q="m&amp;n">Tom &amp; Jerry</a>"#;

    // regular attribute
    assert_eq!(select(html, r#"[href*="&amp;"]"#), ["a#a"]);
    assert_eq!(select(html, r#"[href$="y=2"]"#), ["a#a"]);
    // data attribute (entity-form and decoded-exact both match)
    assert_eq!(select(html, r#"[data-q*="&amp;"]"#), ["a#a"]);
    assert_eq!(select(html, r#"[data-q="m&n"]"#), ["a#a"]);
    // text content
    assert_eq!(select(html, r#"[text*="&amp;"]"#), ["a#a"]);
    assert_eq!(select(html, r#"[text*="Tom & Jerry"]"#), ["a#a"]);
    // a literal entity that is not present must not match
    assert!(select(html, r#"[data-q*="&lt;"]"#).is_empty());
}

#[test]
fn doctype_is_not_a_sibling() {
    // Sibling-counting pseudo-classes and combinators see elements only, so a leading doctype
    // (or comment) does not stop a top-level element from being a first or only child.
    let html = "<!DOCTYPE html><!-- c --><html><body><p>a</p></body></html>";
    assert_eq!(select(html, "html:first-child"), ["html"]);
    assert_eq!(select(html, "html:only-child"), ["html"]);
    assert_eq!(select(html, "html:nth-child(1)"), ["html"]);
    assert_eq!(select(html, "html:first-of-type"), ["html"]);
    let html = "<!DOCTYPE html><p>a</p><div>b</div>";
    assert_eq!(select(html, "p:first-child"), ["p"]);
    assert_eq!(select(html, "p + div"), ["div"]);
}

#[test]
fn universal_selector_matches_elements_only() {
    // The doctype, comments and text are nodes but not elements, and the document root is
    // never an element ancestor/parent — so `* > p` must not match a top-level `<p>`.
    let html =
        "<!DOCTYPE html><!-- c --><p>a</p><div><!-- d --><span>b</span>t<my-el></my-el></div>";
    let cases: &[(&str, &[&str])] = &[
        ("*", &["p", "div", "span", "my-el"]),
        ("div > *", &["span", "my-el"]),
        ("div *", &["span", "my-el"]),
        ("div>*", &["span", "my-el"]),
        ("* *", &["span", "my-el"]),
        ("* > p", &[]),
        ("* p", &[]),
        ("* + *", &["div", "my-el"]),
        ("*:not(p)", &["div", "span", "my-el"]),
        ("p:not(*)", &[]),
        (":is(*)", &["p", "div", "span", "my-el"]),
        (":has(*)", &["div"]),
        ("div:has(> *)", &["div"]),
        ("*, p", &["p", "div", "span", "my-el"]),
        // Without a type selector, other compounds used to match non-elements too.
        (":not(p)", &["div", "span", "my-el"]),
        (":not(p) > p", &[]),
        // `:root` is the top-level element, not the tagless document root.
        (":root", &["p", "div"]),
        (":root span", &["span"]),
        (":root > span", &["span"]),
        (":root > p", &[]),
        // The doctype and comment before `p` are not siblings, so `p` is still first.
        (":first-child", &["p", "span"]),
        (":root:first-child", &["p"]),
    ];
    let inner = HtmlDoc::parse(html).unwrap().dom();
    let cell = HtmlDoc::parse(html).unwrap().dom_ref_cell();
    for (css, expected) in cases {
        assert_eq!(select(html, css), *expected, "{css} (immutable walk)");
        let got: Vec<String> = cell
            .root()
            .select_css(css)
            .unwrap()
            .map(|el| el.tag().to_string())
            .collect();
        // `DomRefCell` cannot lend a custom element's name; it reports the `extended` marker.
        let expected: Vec<_> = expected
            .iter()
            .map(|t| if *t == "my-el" { "extended" } else { t })
            .collect();
        assert_eq!(got, expected, "{css} (DomRefCell walk)");
    }
    assert_eq!(
        inner.root().select_css("*.x, *#y, *[x]").unwrap().count(),
        0
    );
}

#[test]
fn universal_selector_parse() {
    for css in ["*", "div > *", "*:not(p)", "*.a", "a *", "* + *", ":not(*)"] {
        let parsed = parse_css(css).unwrap().to_string();
        // `*` round-trips when it stands alone; beside other parts it is redundant and dropped.
        let expected = css
            .strip_prefix('*')
            .filter(|rest| !rest.is_empty() && !rest.starts_with(' '))
            .unwrap_or(css);
        assert_eq!(parsed, expected);
    }
    for css in ["**", "p*", "*p", ".a*"] {
        assert!(parse_css(css).is_err(), "{css} should not parse");
    }
}
