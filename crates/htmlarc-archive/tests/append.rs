//! In-place append (ADR 0010): round-trips, dedup across old+new keys, metadata
//! continuation, and the crash-recovery contract.

use htmlarc_archive::{
    ArchiveAppender, HtmlArchive, HtmlArchiveBuilder, MetaRef, MetaSchema, MetaType, MetaValue,
    MmapArchive,
};
use htmlarc_dom::prelude::*;

fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "htmlarc_appendtest_{}_{tag}.htmlarc",
        std::process::id()
    ))
}

fn doc(text: &str) -> HtmlDoc {
    HtmlDoc::parse(&format!("<body><p>{text}</p></body>")).unwrap()
}

fn base_archive(path: &std::path::Path) {
    let mut b = HtmlArchiveBuilder::default();
    b.add_html("old1".to_string(), doc("old one"));
    b.add_html("old2".to_string(), doc("old two"));
    b.write_to(path).unwrap();
}

#[test]
fn append_round_trip() {
    let path = temp_path("round_trip");
    base_archive(&path);
    let before = std::fs::metadata(&path).unwrap().len();

    let mut app = ArchiveAppender::open(&path).unwrap();
    assert!(app.add_html("new1".to_string(), doc("new one")).unwrap());
    assert!(app.add_html("new2".to_string(), doc("new two")).unwrap());
    assert_eq!(app.appended(), 2);
    assert_eq!(app.doc_count(), 4);
    app.commit().unwrap();

    // The file grew in place (no rewrite of the body).
    assert!(std::fs::metadata(&path).unwrap().len() > before);

    // Both readers see old and new documents; keyed lookup works across the rebuilt
    // sort index; iteration order is arrival order.
    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 4);
    let keys: Vec<_> = mmap.keys().collect();
    assert_eq!(keys, ["old1", "old2", "new1", "new2"]);
    for key in ["old1", "old2", "new1", "new2"] {
        assert!(mmap.get(key).unwrap().is_some(), "get({key})");
    }
    // Old and new text both inflate correctly (old frames use the original dictionary
    // context, new frames were compressed to match).
    let owned = HtmlArchive::read_from(&path).unwrap();
    assert_eq!(owned.len(), 4);
    assert!(owned.get("new2").is_some());

    std::fs::remove_file(&path).ok();
}

#[test]
fn append_dedups_against_existing_and_new_keys() {
    let path = temp_path("dedup");
    base_archive(&path);

    let mut app = ArchiveAppender::open(&path).unwrap();
    assert!(
        !app.add_html("old1".to_string(), doc("SHOULD BE DROPPED"))
            .unwrap()
    );
    assert!(app.add_html("new".to_string(), doc("kept")).unwrap());
    assert!(!app.add_html("new".to_string(), doc("dup of new")).unwrap());
    assert_eq!(app.appended(), 1);
    app.commit().unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 3);

    std::fs::remove_file(&path).ok();
}

#[test]
fn append_continues_metadata() {
    let path = temp_path("meta");
    let mut b = HtmlArchiveBuilder::default();
    b.set_meta_schema(MetaSchema {
        fields: vec![
            ("url".to_string(), MetaType::Str),
            ("status".to_string(), MetaType::Int),
        ],
    })
    .unwrap();
    b.add_html_with_meta(
        "old".to_string(),
        doc("old"),
        vec![
            Some(MetaValue::Str("https://old".into())),
            Some(MetaValue::Int(200)),
        ],
    )
    .unwrap();
    b.write_to(&path).unwrap();

    let mut app = ArchiveAppender::open(&path).unwrap();
    assert_eq!(app.meta_schema().unwrap().fields.len(), 2);
    app.add_html_with_meta(
        "new".to_string(),
        doc("new"),
        vec![Some(MetaValue::Str("https://new".into())), None],
    )
    .unwrap();
    // Type mismatch is rejected BEFORE the document is stored.
    assert!(
        app.add_html_with_meta(
            "bad".to_string(),
            doc("bad"),
            vec![Some(MetaValue::Int(1)), None],
        )
        .is_err()
    );
    // ... so the key is still free and the appender still consistent.
    app.add_html("bare".to_string(), doc("bare")).unwrap();
    app.commit().unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 3);
    assert_eq!(mmap.meta_value(0, 0), Some(MetaRef::Str("https://old")));
    assert_eq!(mmap.meta_value(1, 0), Some(MetaRef::Str("https://new")));
    assert_eq!(mmap.meta_value(1, 1), None);
    assert_eq!(mmap.meta_value(2, 0), None); // bare add => all-null row

    std::fs::remove_file(&path).ok();
}

#[test]
fn append_to_meta_less_archive_rejects_meta_rows() {
    let path = temp_path("no_meta");
    base_archive(&path);
    let mut app = ArchiveAppender::open(&path).unwrap();
    assert!(app.meta_schema().is_none());
    assert!(
        app.add_html_with_meta("k".to_string(), doc("x"), vec![Some(MetaValue::Int(1))])
            .is_err()
    );
    std::fs::remove_file(&path).ok();
}

/// The crash-recovery contract: an append abandoned mid-flight (writer dropped without
/// commit) leaves the file readable as the PRE-append archive, and the next append heals
/// the abandoned tail.
#[test]
fn abandoned_append_recovers_and_heals() {
    let path = temp_path("recovery");
    base_archive(&path);

    // Abandon an append after documents were streamed but before commit.
    {
        let mut app = ArchiveAppender::open(&path).unwrap();
        app.add_html("lost".to_string(), doc("never committed"))
            .unwrap();
        // drop without commit — buffered bytes may or may not have hit the file; either
        // way the tail is not a valid trailer and the staged header offset takes over.
    }

    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 2, "recovered archive is the pre-append state");
    assert!(mmap.get("lost").unwrap().is_none());
    assert!(HtmlArchive::read_from(&path).is_ok());
    drop(mmap);

    // A fresh append overwrites the abandoned garbage and commits cleanly.
    let mut app = ArchiveAppender::open(&path).unwrap();
    app.add_html("kept".to_string(), doc("committed this time"))
        .unwrap();
    app.commit().unwrap();

    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 3);
    assert!(mmap.get("kept").unwrap().is_some());
    assert!(mmap.get("lost").unwrap().is_none());

    std::fs::remove_file(&path).ok();
}

/// Repeated appends accumulate correctly (each leaves a dead old footer behind; a re-pack
/// via the owned reader reclaims them and preserves everything).
#[test]
fn repeated_appends_then_repack() {
    let path = temp_path("repeat");
    base_archive(&path);

    for i in 0..3 {
        let mut app = ArchiveAppender::open(&path).unwrap();
        app.add_html(format!("gen{i}"), doc(&format!("generation {i}")))
            .unwrap();
        app.commit().unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(mmap.len(), 5);
    drop(mmap);

    let repacked = temp_path("repeat_repacked");
    let n = HtmlArchive::pack_to(&path, &repacked).unwrap();
    assert_eq!(n, 5);
    let smaller = std::fs::metadata(&repacked).unwrap().len();
    let grown = std::fs::metadata(&path).unwrap().len();
    assert!(
        smaller < grown,
        "re-pack reclaims dead footers ({smaller} vs {grown})"
    );
    assert_eq!(MmapArchive::open(&repacked).unwrap().len(), 5);

    std::fs::remove_file(&path).ok();
    std::fs::remove_file(&repacked).ok();
}

/// Model every footer write boundary, including a fully durable footer with its
/// recovery marker not yet cleared. A subsequent append must preserve keys AND bodies.
#[test]
fn interrupted_commit_preserves_document_bodies() {
    use std::io::{Seek, SeekFrom, Write};
    for missing in 0..=104 {
        let path = temp_path(&format!("commit_boundary_{missing}"));
        let mut b = HtmlArchiveBuilder::default();
        b.set_meta_schema(MetaSchema {
            fields: vec![("generation".into(), MetaType::Int)],
        })
        .unwrap();
        b.add_html_with_meta("old".into(), doc("old body"), vec![Some(MetaValue::Int(0))])
            .unwrap();
        b.write_to(&path).unwrap();
        let old_trailer = std::fs::metadata(&path).unwrap().len() - 104;
        let mut app = ArchiveAppender::open(&path).unwrap();
        app.add_html_with_meta("new".into(), doc("new body"), vec![Some(MetaValue::Int(1))])
            .unwrap();
        app.commit().unwrap();
        {
            let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            f.seek(SeekFrom::Start(10)).unwrap();
            f.write_all(&old_trailer.to_le_bytes()[..6]).unwrap();
            f.set_len(f.metadata().unwrap().len() - missing).unwrap();
        }
        let mut app = ArchiveAppender::open(&path).unwrap();
        app.add_html_with_meta(
            "third".into(),
            doc("third body"),
            vec![Some(MetaValue::Int(2))],
        )
        .unwrap();
        app.commit().unwrap();
        let owned = HtmlArchive::read_from(&path).unwrap();
        let mmap = MmapArchive::open(&path).unwrap();
        for (key, expected) in [("old", "old body"), ("third", "third body")] {
            let text: String = owned
                .get(key)
                .unwrap()
                .root()
                .descendants()
                .text_chars()
                .collect();
            assert_eq!(text, expected, "missing={missing}, key={key}");
            let text: String = mmap
                .doc_by_key(key)
                .unwrap()
                .unwrap()
                .root()
                .descendants()
                .text_chars()
                .collect();
            assert_eq!(text, expected, "missing={missing}, key={key}");
        }
        if missing == 0 {
            let text: String = owned
                .get("new")
                .unwrap()
                .root()
                .descendants()
                .text_chars()
                .collect();
            assert_eq!(text, "new body");
            assert_eq!(mmap.meta_value(1, 0), Some(MetaRef::Int(1)));
        } else {
            assert!(owned.get("new").is_none());
        }
        assert_eq!(mmap.meta_value(mmap.len() - 1, 0), Some(MetaRef::Int(2)));
        drop(mmap);
        std::fs::remove_file(path).unwrap();
    }
}

/// A sparse abandoned tail is discarded without loading it into memory.
#[test]
fn append_recovers_large_sparse_tail() {
    use std::io::{Seek, SeekFrom, Write};
    let path = temp_path("sparse_tail");
    base_archive(&path);
    let original_size = std::fs::metadata(&path).unwrap().len();
    {
        let mut f = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        f.seek(SeekFrom::Start(10)).unwrap();
        f.write_all(&(original_size - 104).to_le_bytes()[..6])
            .unwrap();
        f.set_len(original_size + 128 * 1024 * 1024).unwrap();
    }
    let mut app = ArchiveAppender::open(&path).unwrap();
    app.add_html("kept".into(), doc("kept body")).unwrap();
    app.commit().unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() < original_size + 4096);
    assert_eq!(HtmlArchive::read_from(&path).unwrap().len(), 3);
    std::fs::remove_file(path).unwrap();
}
