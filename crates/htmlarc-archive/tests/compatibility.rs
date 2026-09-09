//! Frozen v12 fixture written by the pre-release build at 96cd3b2. Do not
//! regenerate to make a failure pass: a changed reader must preserve its meaning.
use htmlarc_archive::{HtmlArchive, MetaRef, MmapArchive};
use htmlarc_dom::prelude::*;

#[test]
fn reads_frozen_v12_archive() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/v12.bin");
    let owned = HtmlArchive::read_from(&path).unwrap();
    let mmap = MmapArchive::open(&path).unwrap();
    assert_eq!(owned.len(), 2);
    assert_eq!(mmap.keys().collect::<Vec<_>>(), ["café", "empty"]);
    assert_eq!(mmap.meta_value(0, 0), Some(MetaRef::Int(12)));
    assert_eq!(mmap.meta_value(1, 0), None);
    let text: String = owned
        .get("café")
        .unwrap()
        .root()
        .descendants()
        .text_chars()
        .collect();
    assert_eq!(text, "Hello v12café & tea");
    let doc = mmap.doc_by_key("café").unwrap().unwrap();
    assert_eq!(
        doc.root().descendants().text_chars().collect::<String>(),
        text
    );
}
