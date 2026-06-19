//! Profiling driver for the CSS select **walk** over htmlarc's **memory-mapped** read path
//! (and, for comparison, the owned in-memory DOM). Companion to the `select_vs_scraper` bench:
//! the bench gives wall-clock numbers, this binary feeds the profilers so we can see *where*
//! those cycles go.
//!
//! Three drivers share this one binary.
//!
//! samply (unbiased sampling flamegraph) — build with debug symbols and record it:
//! `cargo build -p htmlarc-archive --example profile_walk --profile profiling`, then
//! `samply record ./target/profiling/examples/profile_walk --query class-present --iters 200000`.
//!
//! hotpath (exact per-function call counts + time) — build with the feature and run; the guard
//! prints a report on exit. Use a small `--iters` (counts are exact regardless):
//! `cargo run -p htmlarc-archive --example profile_walk --features hotpath --profile profiling --
//! --query class-present --iters 2000`.
//!
//! Plain `cargo run` gives a quick functional check.
//!
//! `--mode owned` runs the same walk over `HtmlDoc::parse(..).dom()` so the mmap-vs-owned delta
//! can be isolated. `--query all` runs every query in one process (hotpath then aggregates).

use std::hint::black_box;

use htmlarc_archive::{HtmlArchiveBuilder, MmapArchive};
use htmlarc_dom::css::parse_css;
use htmlarc_dom::prelude::{DomRead, HtmlDoc};

/// The same fixture and query set the `select_vs_scraper` bench uses, so numbers line up.
/// Resolved against the crate dir at compile time so the binary runs from any CWD (samply runs
/// the binary directly, not via `cargo`).
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../htmlarc-dom/src/parser/tests/html/fr.serrer.html"
);

/// `(name, css)` — `name` is what `--query` accepts; a raw CSS string is also accepted.
const QUERIES: &[(&str, &str)] = &[
    ("tag", "div"),
    ("class-present", ".vector-menu-content"),
    ("class-absent", ".this-class-does-not-exist"),
    ("multi-class", ".vector-menu-content.vector-menu"),
    ("id", "#vector-toc"),
    ("attr-exact", r#"[role="navigation"]"#),
    ("attr-ci", r#"[typeof="mw:File"]"#),
    ("attr-presence", "[data-word]"),
];

/// Time one query `iters` times over `target`, returning the (warm) match count. Clones the
/// parsed selector each iteration — htmlarc's real per-query cost, since `.select()` consumes
/// and re-resolves the list. Identical body for mmap `Doc` and owned DOM (both `DomRead`).
fn run_query<D: DomRead>(target: &D, css: &str, iters: usize) -> usize {
    let sel = parse_css(css).expect("valid CSS");
    let count = target.root().select(sel.clone()).count();
    for _ in 0..iters {
        black_box(target.root().select(black_box(sel.clone())).count());
    }
    count
}

fn main() {
    // Held for the whole run; prints the per-function profiling report on drop. `functions_limit(0)`
    // shows every measured label rather than truncating to the top N.
    #[cfg(feature = "hotpath")]
    let _hotpath = hotpath::HotpathGuardBuilder::new("profile_walk")
        .functions_limit(0)
        .build();

    let mut query = "class-present".to_string();
    let mut iters: usize = 20_000;
    let mut mode = "mmap".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--query" => query = args.next().expect("--query needs a value"),
            "--iters" => iters = args.next().expect("--iters needs a value").parse().unwrap(),
            "--mode" => mode = args.next().expect("--mode needs a value"),
            other => panic!("unknown arg: {other}"),
        }
    }

    let html = std::fs::read_to_string(FIXTURE).expect("fixture readable");

    // Resolve `--query` to a list of (name, css). A raw CSS string runs as a one-off.
    let selected: Vec<(&str, String)> = if query == "all" {
        QUERIES.iter().map(|(n, c)| (*n, c.to_string())).collect()
    } else if let Some((n, c)) = QUERIES.iter().find(|(n, _)| *n == query) {
        vec![(*n, c.to_string())]
    } else {
        vec![("custom", query.clone())]
    };

    eprintln!("mode={mode}  iters={iters}  query={query}");

    match mode.as_str() {
        "mmap" => {
            // Build a one-doc archive, open it memory-mapped, and hoist the warm `Doc`.
            let path = std::env::temp_dir().join(format!(
                "htmlarc_profile_walk_{}.htmlarc",
                std::process::id()
            ));
            {
                let mut b = HtmlArchiveBuilder::default();
                b.add_html("doc".to_string(), HtmlDoc::parse(&html).unwrap());
                b.build().write_to(&path).unwrap();
            }
            let mmap = MmapArchive::open(&path).unwrap();
            let doc = mmap.doc(0);
            for (name, css) in &selected {
                let n = run_query(&doc, css, iters);
                eprintln!("  {name:<14} {css:<30} matched {n}");
            }
            std::fs::remove_file(&path).ok();
        }
        "owned" => {
            let owned = HtmlDoc::parse(&html).unwrap();
            let dom = owned.dom();
            for (name, css) in &selected {
                let n = run_query(&dom, css, iters);
                eprintln!("  {name:<14} {css:<30} matched {n}");
            }
        }
        other => panic!("unknown --mode {other} (expected mmap|owned)"),
    }
}
