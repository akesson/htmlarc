//! Benchmarks for the **memory-mapped read path**, where a document is *relocated*: its
//! text/comment pool no longer lives inline in the doc blob but in the per-bundle
//! [`BundleStrings`] block, reached lazily through the `StringSource` seam. These mirror the
//! owned-DOM `iteration`/`selectors` benches in `htmlarc-dom` so the extra cost of the relocation
//! machinery (`bundle_of` lookup + `bundle_strings` access + `source_for` + the seam) is visible
//! and guarded against regression. The owned benches isolate the seam over a `Vec`; these isolate
//! it over mmap bytes plus the bundle indirection.
//!
//! [`BundleStrings`]: htmlarc_archive

use std::fs;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_archive::{BUNDLE_CAP, HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::css::{ClassSelector, ComplexSelector, CompoundSelector, SelectorList};
use htmlarc_dom::prelude::{DomRead, HtmlDoc, HtmlFormat};

/// The same 5,589-element fixture the `htmlarc-dom` iteration/selector benches use, so the
/// relocated read path is directly comparable to the owned one.
const FIXTURE: &str = "../htmlarc-dom/src/parser/tests/html/fr.serrer.html";

fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "htmlarc_bench_{}_{tag}.htmlarc",
        std::process::id()
    ))
}

/// One rich document, mmapped. Every read resolves text through the bundle's relocated pool, so
/// these expose the per-document seam overhead vs the owned `iteration`/`selectors`/render benches.
fn single_doc(c: &mut Criterion) {
    let html = fs::read_to_string(FIXTURE).expect("fixture readable from htmlarc-archive dir");
    let path = temp_path("single");
    {
        let mut b = HtmlArchiveBuilder::default();
        b.add_html("doc".to_string(), HtmlDoc::parse(&html).unwrap());
        b.build().write_to(&path).unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();

    c.bench_function(
        "mmap: doc(0) + iterate all 5589 elements (relocated)",
        |b| b.iter(|| mmap.doc(0).root().forwards().count()),
    );

    c.bench_function("mmap: doc(0) + select class (relocated)", |b| {
        b.iter(|| {
            let selector = SelectorList {
                selectors: vec![ComplexSelector {
                    first: CompoundSelector {
                        classes: vec![ClassSelector::new("vector-menu-content")],
                        ..Default::default()
                    },
                    selectors: Vec::new(),
                }],
            };
            mmap.doc(0).root().select(selector).for_each(|_e| {})
        })
    });

    c.bench_function("mmap: doc_by_key + render Raw (relocated)", |b| {
        b.iter(|| {
            mmap.doc_by_key("doc")
                .unwrap()
                .unwrap()
                .to_html(HtmlFormat::Raw)
        })
    });

    fs::remove_file(&path).ok();
}

/// An archive spanning three bundles of small documents, swept end to end. Isolates the
/// per-document bundle resolution (`bundle_of` partition + `bundle_strings` access) that a flat
/// sweep crosses bundle boundaries through — the cost the inline format never paid.
fn multi_bundle_sweep(c: &mut Criterion) {
    let n = BUNDLE_CAP * 2 + 50; // straddle three bundles
    let path = temp_path("sweep");
    {
        let mut b = HtmlArchiveBuilder::default();
        for i in 0..n {
            let html = format!(
                "<body><p class=\"c{}\">text number {i} here</p></body>",
                i % 7
            );
            b.add_html(format!("k{i:06}"), HtmlDoc::parse(&html).unwrap());
        }
        b.build().write_to(&path).unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();
    let len = mmap.len();

    c.bench_function(
        "mmap: sweep + iterate every doc across 3 bundles (relocated)",
        |b| {
            b.iter(|| {
                let mut total = 0usize;
                for i in 0..len {
                    total += mmap.doc(i).root().forwards().count();
                }
                total
            })
        },
    );

    fs::remove_file(&path).ok();
}

criterion_group!(benches, single_doc, multi_bundle_sweep);
criterion_main!(benches);
