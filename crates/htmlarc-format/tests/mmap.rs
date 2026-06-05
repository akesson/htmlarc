//! End-to-end parity tests for the zero-copy memory-mapped archive: a
//! memory-mapped archive must answer every query identically to the owned one.

use htmlarc_dom::prelude::{HtmlDoc, HtmlFormat, HtmlTag};
use htmlarc_format::{HtmlArchive, HtmlArchiveBuilder, MmapArchive};

fn sample_archive() -> HtmlArchive {
    let mut b = HtmlArchiveBuilder::default();
    // ASCII keys so grapheme-count == char-count; varied content to exercise the
    // node blob, the string pool, attributes, data-attributes, and classes.
    b.add_html(
        "gamma".to_string(),
        HtmlDoc::parse(r#"<body><p class="c x">gamma &amp; body</p><!-- c --></body>"#).unwrap(),
    );
    b.add_html(
        "alpha".to_string(),
        HtmlDoc::parse(
            r#"<body><h1 id="a" class="title big" data-k="v">alpha</h1><div><span>nested</span></div></body>"#,
        )
        .unwrap(),
    );
    b.add_html(
        "beta".to_string(),
        HtmlDoc::parse("<body><a href=\"/x\">beta text</a> tail</body>").unwrap(),
    );
    b.build()
}

fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("htmlarc_mmaptest_{}_{tag}.htmlarc", std::process::id()))
}

#[test]
fn mmap_matches_owned() {
    let path = temp_path("parity");
    sample_archive().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    assert_eq!(mmap.len(), owned.len());
    assert_eq!(mmap.keys().collect::<Vec<_>>(), owned.keys().collect::<Vec<_>>());

    for owned_entry in owned.entries() {
        let key = owned_entry.key.as_str();
        let archived = mmap.get(key).expect("key present in mmap archive");

        assert_eq!(archived.key(), key);
        assert_eq!(archived.checksum(), owned_entry.checksum);

        // The whole point: byte-identical rendering, zero-copy vs owned.
        assert_eq!(
            archived.to_html(HtmlFormat::Raw),
            owned_entry.html.to_html(HtmlFormat::Raw),
            "raw render differs for {key}"
        );
        assert_eq!(
            archived.to_html(HtmlFormat::Pretty),
            owned_entry.html.to_html(HtmlFormat::Pretty),
            "pretty render differs for {key}"
        );
    }

    std::fs::remove_file(&path).ok();
}

#[test]
fn mmap_css_select_matches_owned() {
    let path = temp_path("css");
    sample_archive().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    // A CSS query run directly off the mmap must find the same elements (by tag).
    let owned_hits: Vec<HtmlTag> = owned
        .get("alpha")
        .unwrap()
        .root()
        .select_css(".title")
        .unwrap()
        .map(|el| el.tag())
        .collect();
    let mmap_hits: Vec<HtmlTag> = mmap
        .get("alpha")
        .unwrap()
        .root()
        .select_css(".title")
        .unwrap()
        .map(|el| el.tag())
        .collect();

    assert_eq!(owned_hits, mmap_hits);
    assert_eq!(owned_hits, vec![HtmlTag::h1]);

    std::fs::remove_file(&path).ok();
}

#[test]
fn mmap_reads_legacy_headerless_archive() {
    use rkyv::rancor::Error;

    // Simulate a pre-header archive: the raw rkyv payload with no 16-byte header.
    let archive = sample_archive();
    let payload = rkyv::to_bytes::<Error>(&archive.entries).unwrap();
    let path = temp_path("legacy");
    std::fs::write(&path, &payload).unwrap();

    // Both the owned and mmap readers must transparently handle the legacy layout.
    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), owned.len());
    assert_eq!(
        mmap.get("beta").unwrap().to_html(HtmlFormat::Raw),
        owned.get("beta").unwrap().html.to_html(HtmlFormat::Raw)
    );

    std::fs::remove_file(&path).ok();
}

#[test]
fn mmap_rejects_corrupt_archive() {
    let path = temp_path("corrupt");
    sample_archive().write_to(&path).unwrap();

    // Corrupt the rkyv payload near the tail, where the root/relative-pointers live.
    let mut bytes = std::fs::read(&path).unwrap();
    let n = bytes.len();
    for b in bytes[n - 8..].iter_mut() {
        *b ^= 0xFF;
    }
    let bad = path.with_extension("bad");
    std::fs::write(&bad, &bytes).unwrap();

    // Safe validated open must reject it with an Err, not UB or a crash.
    assert!(
        MmapArchive::open(&bad).is_err(),
        "corrupt archive must be rejected"
    );

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&bad).ok();
}

#[test]
fn mmap_archive_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MmapArchive>();
}
