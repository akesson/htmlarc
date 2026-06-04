use htmlarc_dom::css::parse_css;
use proc_macro::TokenStream;

fn selector_code(css: &str) -> String {
    match parse_css(css) {
        Ok(_) => format!("htmlarc_dom::css::parse_css(\"{css}\").unwrap()"),
        Err(e) => panic!("{e} when parsing: {css}"),
    }
}

/// Converts a CSS selector string to a Rust expression that parses the selector.
/// This is mainly for evaluating the CSS selector at compile time.
///
/// Note that due to some macro argument limitations the string cannot use
/// escaped double quotes, but you can use raw strings.
///
/// Allowed:
/// - css!("a\[href*='creativecommon']")
/// - css!(r#"a\[href*='creativecommon']"#)
///
/// Not allowed:
/// - css!("a\[href*=\\"creativecommon\\"]")
/// - css!(r#"a\[href*="creativecommon"]"#)
#[proc_macro]
pub fn css(input: TokenStream) -> TokenStream {
    let css = input.to_string();
    let mut chars = css.chars();
    let c = chars.next().unwrap();

    let css_str = if c == '\"' {
        css[1..css.len() - 1].replace("\\\"", "\"")
    } else if c == 'r' {
        let hash_count = chars.take_while(|c| *c == '#').count();
        let start = hash_count + 2;
        let end = css.len() - hash_count - 1;
        css[start..end].to_string()
    } else {
        css
    };
    let code_str = selector_code(&css_str);

    code_str
        .parse()
        .unwrap_or_else(|_| panic!("Could not parse: {css_str}"))
}

#[test]
fn test_parse_css() {
    let sel_str = "div > span";
    let css_sel = selector_code(sel_str);

    assert_eq!(
        css_sel,
        r###"htmlarc_dom::css::parse_css("div > span").unwrap()"###
    );
}
