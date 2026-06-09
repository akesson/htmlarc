//! End-to-end parity tests for the zero-copy memory-mapped archive: a
//! memory-mapped archive must answer every query identically to the owned one.

use htmlarc_dom::prelude::{HtmlDoc, HtmlFormat, HtmlTag};
use htmlarc_archive::{HtmlArchive, HtmlArchiveBuilder, MmapArchive};

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
        let archived = mmap
            .get(key)
            .expect("valid blob")
            .expect("key present in mmap archive");

        assert_eq!(archived.key(), key);
        assert_eq!(archived.checksum(), owned_entry.checksum);
        // The directory's checksum (read without touching the blob) must agree too.
        assert_eq!(mmap.checksum_for_key(key), Some(owned_entry.checksum));

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
fn empty_archive_round_trips() {
    let path = temp_path("empty");
    HtmlArchiveBuilder::default().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    assert_eq!(owned.len(), 0);
    assert!(owned.is_empty());
    assert_eq!(mmap.len(), 0);
    assert!(mmap.is_empty());
    assert!(mmap.get("anything").unwrap().is_none());
    assert_eq!(mmap.keys().count(), 0);

    std::fs::remove_file(&path).ok();
}

#[test]
fn mmap_rejects_corrupt_footer() {
    let path = temp_path("corrupt_footer");
    sample_archive().write_to(&path).unwrap();

    // Corrupt the very tail — the trailer magic — so the footer can't be bootstrapped.
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
        "corrupt footer must be rejected at open"
    );
    assert!(HtmlArchive::read_from(&bad).is_err());

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&bad).ok();
}

#[test]
fn mmap_rejects_corrupt_blob() {
    let path = temp_path("corrupt_blob");
    sample_archive().write_to(&path).unwrap();

    // The footer (trailer + directory) stays intact, but every document blob is destroyed.
    // `open` still succeeds (it validates only the footer), and a `get` that must materialize
    // a blob surfaces the corruption as an `Err` — never UB, never confused with absence.
    let mut bytes = std::fs::read(&path).unwrap();
    let n = bytes.len();
    let dir_offset = u64::from_le_bytes(bytes[n - 48..n - 40].try_into().unwrap()) as usize;
    for b in bytes[16..dir_offset].iter_mut() {
        *b ^= 0xFF;
    }
    let bad = path.with_extension("bad");
    std::fs::write(&bad, &bytes).unwrap();

    let mmap = MmapArchive::open(&bad).expect("footer is intact, so open succeeds");
    // Keys/len come from the directory and are unaffected.
    assert_eq!(mmap.len(), 3);
    assert!(mmap.keys().any(|k| k == "beta"));
    // Fetching a document validates its (corrupt) blob and returns an Err.
    assert!(
        matches!(mmap.get("beta"), Err(_)),
        "a corrupt blob must surface as Err, not None"
    );
    // The owned reader deserializes every blob up front, so it fails outright.
    assert!(HtmlArchive::read_from(&bad).is_err());

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&bad).ok();
}

#[test]
fn rejects_truncated_file() {
    let path = temp_path("truncated");
    sample_archive().write_to(&path).unwrap();

    // A file too short to even hold a header + trailer must be rejected, not panic-indexed.
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.truncate(40);
    let short = path.with_extension("short");
    std::fs::write(&short, &bytes).unwrap();

    assert!(MmapArchive::open(&short).is_err());
    assert!(HtmlArchive::read_from(&short).is_err());

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&short).ok();
}

#[test]
fn mmap_archive_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MmapArchive>();
}

#[test]
fn archive_trait_and_filter_work_generically() {
    use htmlarc_archive::{Archive, ArchiveEntry, Filter};

    // One generic routine over the shared trait runs against both backings.
    fn keys_with_h1<A: Archive>(archive: &A) -> Vec<String> {
        let filter = Filter::new(vec!["css:h1".to_string()], vec![]).unwrap();
        archive
            .entries_matching(&filter)
            .map(|e| e.key().to_string())
            .collect()
    }

    let path = temp_path("trait_filter");
    sample_archive().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    // Only the `alpha` document has an <h1>; both backings agree via the trait.
    assert_eq!(keys_with_h1(&owned), vec!["alpha".to_string()]);
    assert_eq!(keys_with_h1(&mmap), vec!["alpha".to_string()]);

    // is_empty parity (HtmlArchive gained it; MmapArchive already had it).
    assert!(!owned.is_empty() && !mmap.is_empty());

    std::fs::remove_file(&path).ok();
}
