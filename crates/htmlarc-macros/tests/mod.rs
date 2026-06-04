use htmlarc_macros::css;

#[test]
fn test() {
    let _selector = css!("a[href*='creativecommon']");
    // not supported
    // let _selector = css!("a[href*=\"creativecommon\"]");
    let _selector = css!(r#"a[href*='creativecommon']"#);
    // not supported
    // let _selector = css!(r#"a[href*="creativecommon"]"#);
}
