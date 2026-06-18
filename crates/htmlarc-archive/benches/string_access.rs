//! Reference benchmarks for the **string-read access pattern** — random vs sequential — over the
//! per-bundle relocated text block, the block being prepared for compression.
//!
//! Every document's text/comment pool lives in its bundle's [`BundleStrings`] block, reached
//! through the `StringSource` seam. Today that block is stored *uncompressed*, so a read is a
//! zero-copy slice and **random ≈ sequential** (only mmap page locality differs). That equivalence
//! is exactly the point of this baseline: when the slice-frame + per-bundle-dict compression lands
//! (plugged into the dormant `StringSource::Lazy` seam), a random single-doc read will have to
//! inflate its whole slice while a sequential sweep amortises one inflate per slice — opening a
//! gap these same benches will then quantify. Capturing the "before" here makes that regression
//! measurable rather than invisible.
//!
//! The per-doc work is identical in both directions — walk every text node and consume its chars
//! (`text_chars`), forcing each string through the seam — so the only variable is the visit order.
//!
//! [`BundleStrings`]: htmlarc_archive

use std::fs;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_archive::{BUNDLE_CAP, HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::prelude::{DomIterator, DomRead, HtmlDoc};

/// A spread of real pages — two rich (~5–6k element) articles and two smaller ones — cycled under
/// distinct keys to fill several bundles. Dedup is by key, so repeating the same page text under
/// different keys is fine: it exercises the relocation/seam machinery at bundle scale, which is
/// what the access pattern stresses (the absolute throughput is not a corpus claim).
const FIXTURES: [&str; 4] = [
    "../htmlarc-dom/src/parser/tests/html/en.interest.html",
    "../htmlarc-dom/src/parser/tests/html/fr.serrer.html",
    "../htmlarc-dom/src/html/tests/html/axel.html",
    "../htmlarc-dom/src/html/tests/html/aerodynamik.html",
];

fn temp_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "htmlarc_bench_straccess_{}_{tag}.htmlarc",
        std::process::id()
    ))
}

/// A deterministic permutation of `0..n` (a seeded LCG Fisher–Yates), so the random-order sweep is
/// stable run to run and the two benches differ only in visit order, never in which docs they read.
fn shuffled(n: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..n).collect();
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15; // fixed seed
    for i in (1..n).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let j = (state >> 33) as usize % (i + 1);
        order.swap(i, j);
    }
    order
}

/// Walk every text node of document `i` and consume its characters, forcing each string through the
/// `StringSource` seam. Returns the total byte length so the work cannot be optimised away.
#[inline]
fn read_all_text(mmap: &MmapArchive, i: usize) -> usize {
    mmap.doc(i)
        .root()
        .descendants()
        .text_chars()
        .map(|c| c.len_utf8())
        .sum()
}

/// Build a multi-bundle archive once, then sweep its documents' text in document order (sequential)
/// and in a fixed shuffled order (random). Both do identical per-doc work.
fn string_access(c: &mut Criterion) {
    let htmls: Vec<String> = FIXTURES
        .iter()
        .map(|p| fs::read_to_string(p).unwrap_or_else(|e| panic!("fixture {p}: {e}")))
        .collect();

    let n = BUNDLE_CAP * 2 + 50; // straddle three bundles, like the mmap_read sweep
    let path = temp_path("multi");
    {
        let mut b = HtmlArchiveBuilder::default();
        for i in 0..n {
            let html = &htmls[i % htmls.len()];
            b.add_html(format!("k{i:06}"), HtmlDoc::parse(html).unwrap());
        }
        b.build().write_to(&path).unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();
    let len = mmap.len();
    let order = shuffled(len);

    let mut group = c.benchmark_group("string_access");

    group.bench_function("sequential: read all text of every doc", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for i in 0..len {
                total += read_all_text(&mmap, i);
            }
            total
        })
    });

    group.bench_function("random: read all text of every doc", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for &i in &order {
                total += read_all_text(&mmap, i);
            }
            total
        })
    });

    group.finish();
    fs::remove_file(&path).ok();
}

criterion_group!(benches, string_access);
criterion_main!(benches);
