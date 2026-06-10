use crate::css::tests::helpers::select;

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
