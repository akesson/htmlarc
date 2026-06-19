//! htmlarc vs `scraper` — CSS **select-query** performance on the same document.
//!
//! `scraper` is the de-facto best-in-class Rust option: it parses with `html5ever`
//! and matches with Servo's `selectors` crate (the engine Firefox ships). This bench
//! pits htmlarc's selector engine against it on the shared `fr.serrer.html` fixture.
//!
//! What is timed: ONE `.select(query).count()` over a **pre-parsed** document, with the
//! CSS string already turned into each engine's AST/compiled form OUTSIDE the loop. So
//! document parsing and CSS-string parsing are excluded — only the query walk is measured.
//!
//! Honest asymmetry (see `iters/match_iter.rs`): htmlarc's `.select()` runs a per-document
//! `resolve` pass on EVERY call (binding the selector to the doc's symbol table, then
//! integer-comparing per node). `scraper` compiles a `Selector` once and reuses it across
//! calls (atoms interned at parse time). So htmlarc's number includes a per-call binding
//! step that scraper amortizes away. That is each library's real, public per-query cost.

use std::fs;
use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use htmlarc_dom::prelude::*;
use scraper::{Html, Selector};

const FIXTURE: &str = "src/parser/tests/html/fr.serrer.html";

/// Standard CSS supported identically by both engines. Mirrors the cases in `selectors.rs`.
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

pub fn selectors_vs_scraper(c: &mut Criterion) {
    let doc = fs::read_to_string(FIXTURE).unwrap();

    // Each library parses the document exactly once (NOT timed).
    let ha = HtmlDoc::parse(&doc).unwrap().dom();
    let sc = Html::parse_document(&doc);

    // One-time sanity print: confirm both engines find the same match counts, so we are
    // comparing equal work. (Goes to stderr; criterion's own output is on stdout.)
    eprintln!("\n  match-count parity (htmlarc / scraper):");
    for (name, css) in QUERIES {
        let ha_n = ha.root().select(parse_css(css).unwrap()).count();
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

        group.bench_function("htmlarc", |b| {
            // `.select` consumes the list and re-resolves it against the document each call,
            // so we hand it a fresh clone — this is htmlarc's real per-query cost.
            b.iter(|| black_box(ha.root().select(black_box(ha_sel.clone())).count()))
        });

        group.bench_function("scraper", |b| {
            b.iter(|| black_box(sc.select(black_box(&sc_sel)).count()))
        });

        group.finish();
    }
}

criterion_group!(benches, selectors_vs_scraper);
criterion_main!(benches);
