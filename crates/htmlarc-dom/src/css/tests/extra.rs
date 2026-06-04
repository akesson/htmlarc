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
