//! A/B (perf/linear-mmap-iter): the layout-exploiting **linear** walk vs the tree-walking **walk**,
//! over htmlarc's **memory-mapped** read path (`Doc`).
//!
//! On a contiguous backing the default `forwards()`/`descendants()`/`select()` ARE the linear walk
//! ([`LinearSweep`] — a `u32` counter over the DFS-pre-order blob, no link byte-reads). The `*_walk`
//! methods force the tree-walking `ElementIter` (`VisitedStack` + `find_next` + per-step link reads).
//! Both run the *same* per-node matching/char work, so each A/B delta isolates the iterator cost.
//!
//! Two query shapes are measured:
//!   - **select** (`forwards` from root) — the headline path; tag/class/id/attr selectors.
//!   - **descendants** (`text_content` + a raw descendant count) — exercises the subtree range
//!     (`subtree_end`) that the select path never touches.
//!
//! The two engines are interleaved per group (criterion alternates the `bench_function`s), the
//! protocol the perf notes require for sub-4% deltas given the machine's ±3–4% noise floor. Parity
//! is printed once up front (and enforced by `linear_iter`'s unit tests).

use std::fs;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_archive::{HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::css::parse_css;
use htmlarc_dom::prelude::{DomIterator, DomRead, HtmlDoc};

/// Same fixture as the other select benches, so numbers line up.
const FIXTURE: &str = "../htmlarc-dom/src/parser/tests/html/fr.serrer.html";

/// Topology-only queries (no string decompression on the walk), spanning cheap-match
/// (`tag`/`class absent`) where the iterator dominates, to heavier matches where it does not.
const QUERIES: &[(&str, &str)] = &[
    ("tag", "div"),
    ("class present", ".vector-menu-content"),
    ("class absent", ".this-class-does-not-exist"),
    ("multi-class", ".vector-menu-content.vector-menu"),
    ("id", "#vector-toc"),
    ("attr exact", r#"[role="navigation"]"#),
    ("attr insensitive", r#"[typeof="mw:File"]"#),
    ("attr presence", "[data-word]"),
];

fn temp_path(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "htmlarc_bench_{}_{tag}.htmlarc",
        std::process::id()
    ))
}

pub fn select_linear_vs_stack(c: &mut Criterion) {
    let html = fs::read_to_string(FIXTURE).expect("fixture readable from htmlarc-archive dir");

    let path = temp_path("select_linear_vs_stack");
    {
        let mut b = HtmlArchiveBuilder::default();
        b.add_html("doc".to_string(), HtmlDoc::parse(&html).unwrap());
        b.build().write_to(&path).unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();
    let doc = mmap.doc(0);

    // --- parity (linear vs walk) ---
    eprintln!("\n  select match-count parity (linear / walk):");
    for (name, css) in QUERIES {
        let linear_n = doc.root().select(parse_css(css).unwrap()).count();
        let walk_n = doc.root().select_walk(parse_css(css).unwrap()).count();
        let flag = if linear_n == walk_n { "ok" } else { "MISMATCH" };
        eprintln!("    {name:<16} {css:<28} {linear_n:>5} / {walk_n:<5}  {flag}");
    }
    {
        let linear_d = doc.root().descendants().count();
        let walk_d = doc.root().descendants_walk().count();
        let linear_t: String = doc.root().descendants().text_chars().collect();
        let walk_t: String = doc.root().descendants_walk().text_chars().collect();
        eprintln!(
            "  descendants count linear/walk: {linear_d}/{walk_d}  text eq: {}",
            linear_t == walk_t
        );
    }
    eprintln!();

    // --- select: linear (default) vs walk (tree) ---
    for (name, css) in QUERIES {
        let sel = parse_css(css).unwrap();
        let mut group = c.benchmark_group(format!("select {name} [{css}]"));
        group.bench_function("linear", |b| {
            b.iter(|| black_box(doc.root().select(black_box(sel.clone())).count()))
        });
        group.bench_function("walk", |b| {
            b.iter(|| black_box(doc.root().select_walk(black_box(sel.clone())).count()))
        });
        group.finish();
    }

    // --- descendants: the subtree range the select path never exercises ---
    {
        let mut group = c.benchmark_group("descendants count [root]");
        group.bench_function("linear", |b| {
            b.iter(|| black_box(doc.root().descendants().count()))
        });
        group.bench_function("walk", |b| {
            b.iter(|| black_box(doc.root().descendants_walk().count()))
        });
        group.finish();
    }
    {
        let mut group = c.benchmark_group("text_content [root]");
        group.bench_function("linear", |b| {
            b.iter(|| black_box(doc.root().descendants().text_chars().collect::<String>()))
        });
        group.bench_function("walk", |b| {
            b.iter(|| {
                black_box(
                    doc.root()
                        .descendants_walk()
                        .text_chars()
                        .collect::<String>(),
                )
            })
        });
        group.finish();
    }

    fs::remove_file(&path).ok();
}

criterion_group!(benches, select_linear_vs_stack);
criterion_main!(benches);
