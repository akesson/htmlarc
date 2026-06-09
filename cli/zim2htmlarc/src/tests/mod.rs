use std::collections::HashSet;
use std::path::PathBuf;

use zim::{MimeType, Namespace};

use crate::export::{html_mime, is_content, key_for, nfc, parse_wordlist};

#[test]
fn nfc_normalizes_to_precomposed() {
    // "e" + combining acute accent -> precomposed "é".
    assert_eq!(nfc("e\u{0301}"), "\u{00e9}");
    assert_eq!(nfc("already nfc"), "already nfc");
}

#[test]
fn html_mime_accepts_only_text_html() {
    assert!(html_mime(&MimeType::Type("text/html".into())));
    assert!(html_mime(&MimeType::Type(
        "text/html; charset=utf-8".into()
    )));
    assert!(!html_mime(&MimeType::Type("image/png".into())));
    assert!(!html_mime(&MimeType::Type("text/css".into())));
    assert!(!html_mime(&MimeType::Redirect));
}

#[test]
fn content_namespaces_cover_both_schemes() {
    assert!(is_content(&Namespace::Articles)); // old scheme ('A')
    assert!(is_content(&Namespace::UserContent)); // new scheme ('C')
    assert!(!is_content(&Namespace::Metadata));
    assert!(!is_content(&Namespace::FulltextIndex));
}

#[test]
fn key_falls_back_to_url_when_title_empty() {
    assert_eq!(
        key_for("Climate change", "Climate_change"),
        "Climate change"
    );
    assert_eq!(key_for("", "Some_Slug"), "Some_Slug"); // empty title -> url slug
    assert_eq!(key_for("Cafe\u{0301}", "x"), "Caf\u{00e9}"); // still NFC-normalized
}

#[test]
fn wordlist_is_nfc_normalized_deduped_and_skips_blanks() {
    // "Cafe" + combining acute, a blank line, a duplicate, and a plain word.
    let set: HashSet<String> = parse_wordlist("Cafe\u{0301}\n\nCafe\u{0301}\nplain\n");
    assert!(set.contains("Caf\u{00e9}")); // normalized to precomposed
    assert!(set.contains("plain"));
    assert_eq!(set.len(), 2);
}

/// End-to-end against a real ZIM. Ignored by default because no `.zim` fixture is committed
/// (the openzim test suite is unlicensed and can't be redistributed here). To run it, fetch a
/// small ZIM first with `cli/zim2htmlarc/fetch-testdata.sh`, then:
///   cargo nextest run -p zim2htmlarc --run-ignored all
/// Override the ZIM path with the `ZIM2HTMLARC_TEST_ZIM` env var.
#[test]
#[ignore = "needs a local ZIM; run cli/zim2htmlarc/fetch-testdata.sh first"]
fn export_reads_a_real_zim() {
    let zim_path = std::env::var_os("ZIM2HTMLARC_TEST_ZIM")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/test.zim"));
    assert!(
        zim_path.exists(),
        "no test ZIM at {} — set ZIM2HTMLARC_TEST_ZIM or run cli/zim2htmlarc/fetch-testdata.sh",
        zim_path.display()
    );

    let out = std::env::temp_dir().join("zim2htmlarc-e2e.htmlarc");
    crate::export::run(crate::args::Export {
        file: zim_path,
        output: out.clone(),
        list: None,
    })
    .expect("export should succeed");

    let arch = htmlarc_archive::HtmlArchive::read_from(&out).expect("archive should load");
    assert!(!arch.is_empty(), "expected at least one article");
}
