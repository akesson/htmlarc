//! htmlarc (mmap'd) vs `scraper` — CSS **select-query** performance.
//!
//! Companion to `htmlarc-dom/benches/selectors_vs_scraper.rs` (which compares the *owned*
//! in-memory DOM). Here the document is queried over htmlarc's **memory-mapped** read path:
//! `MmapArchive` → `Doc` → `.root().select(..)`, reading the topology zero-copy from the map.
//! `scraper` is the best-in-class Rust baseline (html5ever parse + Servo's `selectors` engine).
//!
//! What is timed: ONE `.select(query).count()` over the **warm, in-memory** map, with the CSS
//! string already turned into each engine's form OUTSIDE the loop. The map is written then
//! opened in-process, so its pages are resident — this measures warm-map CPU cost, not
//! cold-disk page-fault latency. All 8 queries are topology-only (tag/class/id/attr), so the
//! mmap walk performs NO string decompression.
//!
//! Asymmetry (same as the owned bench): htmlarc's `.select()` re-runs a per-document `resolve`
//! pass on every call, so we hand it a fresh clone each iteration — its real per-query cost.
//! `scraper` compiles a `Selector` once and reuses it.

use std::fs;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_archive::{HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::css::parse_css;
use htmlarc_dom::prelude::{DomRead, HtmlDoc};
use scraper::{Html, Selector};

/// The same fixture the owned-DOM and mmap_read benches use, so the numbers line up.
const FIXTURE: &str = "../htmlarc-dom/src/parser/tests/html/fr.serrer.html";

/// Standard CSS supported identically by both engines (mirrors the owned bench).
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

pub fn select_vs_scraper(c: &mut Criterion) {
    let html = fs::read_to_string(FIXTURE).expect("fixture readable from htmlarc-archive dir");

    // Build a one-doc archive, then open it memory-mapped (NOT timed).
    let path = temp_path("select_vs_scraper");
    {
        let mut b = HtmlArchiveBuilder::default();
        b.add_html("doc".to_string(), HtmlDoc::parse(&html).unwrap());
        b.build().write_to(&path).unwrap();
    }
    let mmap = MmapArchive::open(&path).unwrap();
    // Warm, in-memory map; hoist the doc so we measure the select walk, not repeated lookup.
    let doc = mmap.doc(0);

    // scraper parses once (NOT timed).
    let sc = Html::parse_document(&html);

    // One-time parity print: confirm both engines find the same matches (equal work).
    eprintln!("\n  match-count parity (htmlarc-mmap / scraper):");
    for (name, css) in QUERIES {
        let ha_n = doc.root().select(parse_css(css).unwrap()).count();
        let sc_n = sc.select(&Selector::parse(css).unwrap()).count();
        let flag = if ha_n == sc_n { "ok" } else { "MISMATCH" };
        eprintln!("    {name:<16} {css:<28} {ha_n:>5} / {sc_n:<5}  {flag}");
    }
    eprintln!();

    for (name, css) in QUERIES {
        // Selector prepared once per engine (string -> AST / compiled form), NOT timed.
        let ha_sel = parse_css(css).unwrap();
        let sc_sel = Selector::parse(css).unwrap();

        let mut group = c.benchmark_group(format!("{name} [{css}]"));

        group.bench_function("htmlarc-mmap", |b| {
            // `.select` consumes + re-resolves the list each call, so clone per iter.
            b.iter(|| black_box(doc.root().select(black_box(ha_sel.clone())).count()))
        });

        group.bench_function("scraper", |b| {
            b.iter(|| black_box(sc.select(black_box(&sc_sel)).count()))
        });

        group.finish();
    }

    fs::remove_file(&path).ok();
}

criterion_group!(benches, select_vs_scraper);
criterion_main!(benches);
