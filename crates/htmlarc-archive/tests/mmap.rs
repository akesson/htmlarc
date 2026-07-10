//! End-to-end parity tests for the zero-copy memory-mapped archive: a
//! memory-mapped archive must answer every query identically to the owned one.

use htmlarc_archive::{
    BUNDLE_CAP, DocBundle, HtmlArchive, HtmlArchiveBuilder, HtmlEntry, MmapArchive, OwnedDoc,
};
use htmlarc_dom::prelude::{DomIterator, DomRead, HtmlDoc, HtmlFormat, HtmlTag, OwnedSelectorList};

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

        // The whole point: byte-identical rendering, zero-copy vs owned. `doc_by_key` binds the
        // document to its bundle's relocated text.
        let doc = mmap
            .doc_by_key(key)
            .expect("valid blob")
            .expect("key present in mmap archive");
        assert_eq!(
            doc.to_html(HtmlFormat::Raw),
            owned_entry.html.to_html(HtmlFormat::Raw),
            "raw render differs for {key}"
        );
        assert_eq!(
            doc.to_html(HtmlFormat::Pretty),
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
        .doc_by_key("alpha")
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
    // The trailer is the last 88 bytes; doc_table_offset is its first field (bytes [0..8]). The
    // body — document blobs interleaved with per-bundle string blocks — spans
    // [HEADER_LEN, doc_table_offset); destroying it corrupts every document blob.
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

/// The whole point of [`OwnedDoc`]: a `'static + Send + Sync` handle (what a Python binding or
/// an async server must hold) that answers every query identically to the borrowed [`Doc`].
#[test]
fn owned_doc_matches_borrowed_and_crosses_threads() {
    use std::sync::Arc;

    let path = temp_path("owned_doc");
    sample_archive().write_to(&path).unwrap();
    let archive = Arc::new(MmapArchive::open(&path).unwrap());

    // The handle is what lifetimes forbid `Doc` from being.
    fn assert_handle<T: Send + Sync + 'static>(_: &T) {}

    // Parity with the borrowed path, for every document and both render modes.
    for pos in 0..archive.len() {
        let owned = OwnedDoc::new(archive.clone(), pos).unwrap();
        assert_handle(&owned);
        let borrowed = archive.doc(pos);
        assert_eq!(owned.key(), archive.key_at(pos));
        assert_eq!(owned.position(), pos);
        assert_eq!(owned.checksum(), archive.checksum_at(pos));
        assert_eq!(
            owned.to_html(HtmlFormat::Raw),
            borrowed.to_html(HtmlFormat::Raw)
        );
        assert_eq!(
            owned.to_html(HtmlFormat::Pretty),
            borrowed.to_html(HtmlFormat::Pretty)
        );
    }

    // Keyed lookup: present and absent.
    let doc = OwnedDoc::by_key(archive.clone(), "alpha").unwrap().unwrap();
    assert!(
        OwnedDoc::by_key(archive.clone(), "no-such-key")
            .unwrap()
            .is_none()
    );

    // The handle outlives every borrow of the archive: move it (and the last Arc) to another
    // thread together with a compiled selector — the exact shape a binding holds — and query
    // there. Selecting, tag reads, and text all resolve.
    drop(archive);
    let selector = OwnedSelectorList::parse(".title").unwrap();
    let from_thread = std::thread::spawn(move || {
        let tags: Vec<HtmlTag> = doc
            .root()
            .select(selector.list().clone())
            .map(|el| el.tag())
            .collect();
        let text: String = doc.root().descendants().text_chars().collect();
        (tags, text)
    })
    .join()
    .unwrap();
    assert_eq!(from_thread.0, vec![HtmlTag::h1]);
    assert_eq!(from_thread.1, "alphanested");

    std::fs::remove_file(&path).ok();
}

/// An [`OwnedDoc`] in a *later* bundle must resolve its own slot's text (the bundle/slot
/// arithmetic is the part a single-bundle archive cannot exercise).
#[test]
fn owned_doc_resolves_across_bundle_boundaries() {
    use std::sync::Arc;

    let n = BUNDLE_CAP + 3; // two bundles: one full + a partial tail
    let mut b = HtmlArchiveBuilder::default();
    for i in 0..n {
        let html = format!("<body><p>text {i}</p></body>");
        b.add_html(format!("key{i:05}"), HtmlDoc::parse(&html).unwrap());
    }
    let path = temp_path("owned_doc_bundles");
    b.write_to(&path).unwrap();
    let archive = Arc::new(MmapArchive::open(&path).unwrap());

    // One document per bundle, plus the last: each must read its own text, not slot 0's.
    for pos in [0, 1, BUNDLE_CAP - 1, BUNDLE_CAP, BUNDLE_CAP + 2] {
        let doc = OwnedDoc::new(archive.clone(), pos).unwrap();
        let key = doc.key().to_string();
        let i: usize = key.trim_start_matches("key").parse().unwrap();
        let text: String = doc.root().descendants().text_chars().collect();
        assert_eq!(text, format!("text {i}"), "wrong text for {key} at {pos}");
    }

    std::fs::remove_file(&path).ok();
}

#[test]
fn archive_trait_and_filter_work_generically() {
    use htmlarc_archive::{Archive, Filter};

    // One generic routine over the shared trait runs against both backings.
    fn keys_with_h1<A: Archive>(archive: &A) -> Vec<String> {
        let filter = Filter::new(vec!["css:h1".to_string()], vec![]).unwrap();
        archive
            .entries_matching(&filter)
            .into_iter()
            .map(|k| k.to_string())
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

/// The load-bearing relocation test: with the per-document text pool moved out into each bundle's
/// string block, every document must still resolve *its own* text — across bundle boundaries, by
/// position and by key, identically for the owned and memory-mapped backings.
#[test]
fn relocated_strings_round_trip_across_bundle_boundaries() {
    let n = BUNDLE_CAP * 2 + 3; // three bundles: two full + a partial tail
    let mut b = HtmlArchiveBuilder::default();
    for i in 0..n {
        // Distinct per-document text so a mis-bound bundle or slot (off-by-one in the offset
        // table) renders the wrong content and fails loudly.
        let html = format!("<p class=\"c{i}\">content-{i}</p>");
        b.add_html(format!("k{i:05}"), HtmlDoc::parse(&html).unwrap());
    }
    let path = temp_path("relocate_boundaries");
    b.write_to(&path).unwrap();

    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.bundle_count(), 3, "expected two full bundles + a tail");
    assert_eq!(mmap.len(), n);
    assert_eq!(owned.len(), n);

    for i in [
        0,
        BUNDLE_CAP - 1,
        BUNDLE_CAP,
        BUNDLE_CAP + 1,
        2 * BUNDLE_CAP,
        n - 1,
    ] {
        let want = format!("content-{i}");
        let by_pos = mmap.doc(i).to_html(HtmlFormat::Raw);
        assert!(
            by_pos.contains(&want),
            "doc {i}: wrong relocated text: {by_pos}"
        );

        // Owned (text re-attached at load) and mmap (bound from the bundle block) must agree.
        assert_eq!(
            by_pos,
            owned[i].html.to_html(HtmlFormat::Raw),
            "doc {i}: owned vs mmap render differ"
        );

        // Keyed lookup resolves to the same document (it locates the bundle from the position).
        let by_key = mmap
            .doc_by_key(&format!("k{i:05}"))
            .unwrap()
            .unwrap()
            .to_html(HtmlFormat::Raw);
        assert_eq!(by_key, by_pos, "doc {i}: keyed vs positional render differ");
    }

    std::fs::remove_file(&path).ok();
}

/// Materialising an archived (relocated) document into an owned, editable `DomInner` must pull its
/// text back out of the bundle block — the archived→owned `repackage` boundary.
#[test]
fn mmap_doc_repackage_materializes_relocated_text() {
    let path = temp_path("repackage");
    sample_archive().write_to(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();

    let doc = mmap.doc_by_key("alpha").unwrap().unwrap();
    let archived_html = doc.to_html(HtmlFormat::Raw);
    let owned = doc.repackage();
    assert_eq!(
        owned.to_html(HtmlFormat::Raw),
        archived_html,
        "repackaged owned render must match the archived render (text materialized from bundle)"
    );
    assert!(!archived_html.is_empty(), "alpha should render some text");

    std::fs::remove_file(&path).ok();
}

/// Block-split string frames (format v11, ADR 0008): documents whose pools span several ~16 KiB
/// blocks — one as many small text nodes, one as a single oversized node — plus a text-free
/// document between them, must round-trip byte-identically through every read path, and a
/// selective read must inflate only the blocks it touches.
#[test]
fn multi_block_documents_round_trip() {
    use std::sync::Arc;

    // ~50 KiB across 50 distinct paragraphs → several blocks cut at node boundaries.
    let many_nodes: String = (0..50)
        .map(|i| format!("<p id=\"p{i}\">{}</p>", format!("para-{i}-").repeat(128)))
        .collect();
    // A single text node well past the 16 KiB block target → one oversized block.
    let giant_node = format!("<h1>{}</h1>", "z".repeat(20_000));

    let mut b = HtmlArchiveBuilder::default();
    b.add_html(
        "many".to_string(),
        HtmlDoc::parse(&format!("<body>{many_nodes}</body>")).unwrap(),
    );
    b.add_html(
        "empty".to_string(),
        HtmlDoc::parse("<body><div id=\"nada\"></div></body>").unwrap(),
    );
    b.add_html(
        "giant".to_string(),
        HtmlDoc::parse(&format!("<body>{giant_node}</body>")).unwrap(),
    );
    let path = temp_path("multi_block");
    b.build().write_to(&path).unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    // The cutting actually happened: more blocks than documents (the 50 KiB doc alone must
    // split), while the giant single node stays one (oversized) block.
    let strings = mmap.bundle_strings(0).unwrap();
    assert!(
        strings.block_count() > 3,
        "expected the 50 KiB many-node pool to split into several blocks, got {} blocks total",
        strings.block_count()
    );

    // Whole-document render parity across borrowed, owned-handle, and rehydrated reads.
    let owned_archive = HtmlArchive::read_from(&path).unwrap();
    let archive = Arc::new(MmapArchive::open(&path).unwrap());
    for pos in 0..archive.len() {
        let borrowed = archive.doc(pos);
        let handle = OwnedDoc::new(archive.clone(), pos).unwrap();
        let want = owned_archive[pos].html.to_html(HtmlFormat::Raw);
        assert_eq!(borrowed.to_html(HtmlFormat::Raw), want, "doc {pos}");
        assert_eq!(handle.to_html(HtmlFormat::Raw), want, "doc {pos}");
        // repackage crosses the materialize (concat-all-blocks) path.
        assert_eq!(
            handle.repackage().to_html(HtmlFormat::Raw),
            want,
            "doc {pos}"
        );
    }

    // A selective read: text of one late paragraph (deep in the last block of "many").
    let doc = archive.doc(0);
    let hits: Vec<String> = doc
        .root()
        .select_css("#p49")
        .unwrap()
        .map(|el| el.descendants().text_chars().collect())
        .collect();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].starts_with("para-49-"));
    assert_eq!(hits[0].len(), "para-49-".len() * 128);

    std::fs::remove_file(&path).ok();
}

/// A text filter over a block-split archive: `entries_matching` binds documents through the
/// per-bundle arena, and text matching must read the right block.
#[test]
fn text_filter_matches_across_blocks() {
    use htmlarc_archive::{Archive, Filter};

    let many_nodes: String = (0..50)
        .map(|i| format!("<p>{}</p>", format!("w{i}w ").repeat(256)))
        .collect();
    let mut b = HtmlArchiveBuilder::default();
    b.add_html(
        "many".to_string(),
        HtmlDoc::parse(&format!("<body>{many_nodes}</body>")).unwrap(),
    );
    b.add_html(
        "plain".to_string(),
        HtmlDoc::parse("<body><p>nothing to see</p></body>").unwrap(),
    );
    let path = temp_path("text_filter_blocks");
    b.build().write_to(&path).unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    // "w49w" appears only in the last paragraph, deep in the last block of the split pool.
    let filter = Filter::new(vec![r#"css:p[text*="w49w"]"#.to_string()], vec![]).unwrap();
    assert_eq!(mmap.entries_matching(&filter), vec!["many"]);

    std::fs::remove_file(&path).ok();
}

/// A *mutated* document reaching the writer: `replace_text` pushes replacement bytes at the pool
/// end, leaving dead bytes mid-pool, node ranges out of document order, and (for `""`) an empty
/// range at the very end of the pool. Block cutting must cope and the round-trip stay
/// byte-correct.
#[test]
fn mutated_document_round_trips_through_blocks() {
    use htmlarc_dom::prelude::HtmlElement;

    let cell = HtmlDoc::parse(r#"<body><p id="a"></p><p id="b"></p><p id="c"></p></body>"#)
        .unwrap()
        .dom_ref_cell();
    {
        let root = cell.root();
        let ps: Vec<_> = root.select_css("p").unwrap().collect();
        assert_eq!(ps.len(), 3);
        let ta = ps[0].append_text_child("alpha-original");
        let tb = ps[1].append_text_child("beta-original");
        let tc = ps[2].append_text_child("gamma-original");
        // Replace in reverse document order, so the live ranges land in the pool out of
        // document order (tc's replacement sits before tb's) with the dead originals mid-pool.
        HtmlElement::new(&cell, tc).replace_text("gammaNEW");
        // tb's replacement is bigger than a block, forcing a cut among the disordered ranges.
        HtmlElement::new(&cell, tb).replace_text(&"y".repeat(20_000));
        // And an empty replacement last: a `len..len` range at the very end of the pool.
        HtmlElement::new(&cell, ta).replace_text("");
    }
    let doc: HtmlDoc = cell.with_mut(std::mem::take).into();

    let mut b = HtmlArchiveBuilder::default();
    b.add_html("mutated".to_string(), doc);
    let path = temp_path("mutated_blocks");
    b.build().write_to(&path).unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    let owned = HtmlArchive::read_from(&path).unwrap();
    let read = mmap.doc_by_key("mutated").unwrap().unwrap();
    let html = read.to_html(HtmlFormat::Raw);
    assert_eq!(html, owned[0].html.to_html(HtmlFormat::Raw));
    assert!(html.contains("gammaNEW"));
    assert!(html.contains(&"y".repeat(20_000)));
    assert!(
        !html.contains("original"),
        "replaced text must not resurface"
    );

    std::fs::remove_file(&path).ok();
}
