//! End-to-end parity tests for the zero-copy memory-mapped archive: a
//! memory-mapped archive must answer every query identically to the owned one.

use htmlarc_archive::{
    BUNDLE_CAP, DocBundle, HtmlArchive, HtmlArchiveBuilder, HtmlEntry, MmapArchive,
};
use htmlarc_dom::prelude::{HtmlDoc, HtmlFormat, HtmlTag};

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
    std::env::temp_dir().join(format!(
        "htmlarc_mmaptest_{}_{tag}.htmlarc",
        std::process::id()
    ))
}

#[test]
fn mmap_matches_owned() {
    let path = temp_path("parity");
    sample_archive().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    assert_eq!(mmap.len(), owned.len());
    assert_eq!(
        mmap.keys().collect::<Vec<_>>(),
        owned.keys().collect::<Vec<_>>()
    );

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

    // The footer (trailer + doc/bundle tables + sort index) stays intact, but every document
    // blob is destroyed. `open` still succeeds (it validates only the footer), and a `get` that
    // must materialize a blob surfaces the corruption as an `Err` — never UB, never absence.
    let mut bytes = std::fs::read(&path).unwrap();
    let n = bytes.len();
    // v4 trailer is the last 88 bytes; doc_table_offset is its first field (bytes [0..8]). The
    // document blobs span [HEADER_LEN, doc_table_offset) (the per-bundle data region is empty).
    let doc_table_offset = u64::from_le_bytes(bytes[n - 88..n - 80].try_into().unwrap()) as usize;
    for b in bytes[16..doc_table_offset].iter_mut() {
        *b ^= 0xFF;
    }
    let bad = path.with_extension("bad");
    std::fs::write(&bad, &bytes).unwrap();

    let mmap = MmapArchive::open(&bad).expect("footer is intact, so open succeeds");
    // Keys/len come from the footer doc table and are unaffected.
    assert_eq!(mmap.len(), 3);
    assert!(mmap.keys().any(|k| k == "beta"));
    // Fetching a document validates its (corrupt) blob and returns an Err.
    assert!(
        mmap.get("beta").is_err(),
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
fn multi_bundle_round_trips() {
    // More than one bundle's worth of documents, so the writer seals full bundles and a partial
    // tail — exercising the bundle table, cross-boundary positional access, and keyed lookup.
    let n = BUNDLE_CAP * 2 + 5;
    let mut b = HtmlArchiveBuilder::default();
    for i in 0..n {
        b.add_html(
            format!("doc{i:08}"),
            HtmlDoc::parse(&format!("<p>{i}</p>")).unwrap(),
        );
    }
    let path = temp_path("multibundle");
    b.write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    let expected_bundles = n.div_ceil(BUNDLE_CAP);
    assert_eq!(owned.len(), n);
    assert_eq!(mmap.len(), n);
    assert_eq!(owned.bundles().len(), expected_bundles);
    assert_eq!(mmap.bundle_count(), expected_bundles);

    // Bundles must tile [0, n): every position belongs to exactly one bundle, in order.
    let mut covered = 0;
    for bi in 0..mmap.bundle_count() {
        let r = mmap.bundle_range(bi);
        assert_eq!(
            r.start, covered,
            "bundle {bi} starts where the previous ended"
        );
        assert!(r.end <= n);
        covered = r.end;
    }
    assert_eq!(covered, n, "bundles cover every document");

    // Keyed lookup resolves across bundle boundaries; positional access is bundle→doc
    // (== insertion) order.
    for &i in &[0usize, BUNDLE_CAP - 1, BUNDLE_CAP, BUNDLE_CAP + 1, n - 1] {
        let key = format!("doc{i:08}");
        assert!(owned.get(&key).is_some(), "owned get {key}");
        assert!(mmap.get(&key).unwrap().is_some(), "mmap get {key}");
        assert_eq!(owned[i].key, key, "owned positional {i}");
        assert_eq!(mmap.key_at(i), key, "mmap positional {i}");
        assert_eq!(mmap.checksum_at(i), owned[i].checksum);
    }
    assert!(owned.get("absent").is_none());
    assert!(mmap.get("absent").unwrap().is_none());

    std::fs::remove_file(&path).ok();
}

#[test]
fn position_for_key_parity() {
    // The keyed-search fast path resolves a word-list straight to flat positions via the sort
    // index; owned and mmap must agree, the position must round-trip to the key, and an absent
    // key must be `None` (not a panic or a wrong hit).
    let path = temp_path("position_for_key");
    sample_archive().write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    for key in ["gamma", "alpha", "beta"] {
        let oi = owned.position_for_key(key).expect("owned position");
        let mi = mmap.position_for_key(key).expect("mmap position");
        assert_eq!(oi, mi, "owned/mmap position disagree for {key}");
        assert_eq!(owned[oi].key, key, "owned position round-trips");
        assert_eq!(mmap.key_at(mi), key, "mmap position round-trips");
    }

    assert_eq!(owned.position_for_key("absent"), None);
    assert_eq!(mmap.position_for_key("absent"), None);

    std::fs::remove_file(&path).ok();
}

#[test]
fn explicit_bundle_boundaries_round_trip() {
    // Irregularly-sized bundles — as the ZIM export's cluster-aligned runs produce — must survive
    // write→read verbatim, not be re-chunked at BUNDLE_CAP. (write_to seals each in-memory bundle.)
    let sizes = [3usize, 1, 5, 2, 4];
    let mut n = 0usize;
    let bundles: Vec<DocBundle> = sizes
        .iter()
        .map(|&sz| {
            let entries: Vec<HtmlEntry> = (0..sz)
                .map(|_| {
                    let key = format!("k{n:05}");
                    n += 1;
                    HtmlEntry::new(key, HtmlDoc::parse("<p>x</p>").unwrap())
                })
                .collect();
            DocBundle::from_entries(entries)
        })
        .collect();
    let total: usize = sizes.iter().sum();

    let archive = HtmlArchive::from_bundles(bundles);
    let path = temp_path("irregular_bundles");
    archive.write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    assert_eq!(owned.len(), total);
    assert_eq!(mmap.len(), total);
    assert_eq!(owned.bundles().len(), sizes.len(), "bundle count preserved");
    assert_eq!(mmap.bundle_count(), sizes.len());

    // Each bundle keeps its exact size and tiles [0, total) in order.
    let mut covered = 0;
    for (bi, &sz) in sizes.iter().enumerate() {
        let r = mmap.bundle_range(bi);
        assert_eq!(
            r.start, covered,
            "bundle {bi} starts where the previous ended"
        );
        assert_eq!(r.len(), sz, "bundle {bi} keeps its size");
        covered = r.end;
    }
    assert_eq!(covered, total, "bundles cover every document");

    std::fs::remove_file(&path).ok();
}

#[test]
fn rejects_v3_archive() {
    // A file with the right magic but the previous format version must be rejected up front with
    // a clear "re-pack to upgrade" error — never misread as a v4 archive.
    let mut bytes = vec![0u8; 16];
    bytes[0..8].copy_from_slice(b"HTMLARC1");
    bytes[8] = 3; // legacy version byte
    let path = temp_path("v3");
    std::fs::write(&path, &bytes).unwrap();

    let err = MmapArchive::open(&path)
        .err()
        .expect("a v3 archive must be rejected");
    assert!(
        err.to_string().contains("version 3"),
        "error should name the unsupported version: {err}"
    );
    assert!(HtmlArchive::read_from(&path).is_err());

    std::fs::remove_file(&path).ok();
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
