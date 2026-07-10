//! Throwaway benchmark: single-core native floor for archive re-query sweeps.
//! Usage: cargo run --release -p htmlarc-archive --example query_floor -- <path.htmlarc>

use std::sync::Arc;
use std::time::Instant;

use htmlarc_archive::{MmapArchive, OwnedDoc};
use htmlarc_dom::prelude::*;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: query_floor <path.htmlarc>");
    let arc = Arc::new(MmapArchive::open(&path).expect("open archive"));
    let n_docs = arc.len();
    println!("{n_docs} docs");

    // Doc-handle floor: create every OwnedDoc, touch nothing else.
    for rep in 0..3 {
        let t0 = Instant::now();
        let mut n = 0usize;
        for pos in 0..n_docs {
            let doc = OwnedDoc::new(arc.clone(), pos).expect("doc");
            n += doc.key().len();
        }
        println!(
            "create-only                rep{rep} keybytes={n} create={:8.1?}",
            t0.elapsed()
        );
    }

    for (src, is_attr) in [
        ("a[href]", true),
        ("h1, h2, h3", false),
        ("table tr td:first-child", false),
    ] {
        let sel = OwnedSelectorList::parse(src).expect("selector");

        for rep in 0..3 {
            // Pass 1: select only, count matches (no extraction).
            let t0 = Instant::now();
            let mut n = 0usize;
            for pos in 0..n_docs {
                let doc = OwnedDoc::new(arc.clone(), pos).expect("doc");
                let root = HtmlElement::new(&doc, NodeIndex::ROOT);
                n += root.select(sel.list().clone()).count();
            }
            let select = t0.elapsed();

            // Pass 2: select + extract (attr value or text content), like select_attr/select_text.
            let t1 = Instant::now();
            let mut bytes = 0usize;
            for pos in 0..n_docs {
                let doc = OwnedDoc::new(arc.clone(), pos).expect("doc");
                let root = HtmlElement::new(&doc, NodeIndex::ROOT);
                if is_attr {
                    for e in root.select(sel.list().clone()) {
                        bytes += e.get_attribute("href").map_or(0, |s| s.len());
                    }
                } else {
                    for e in root.select(sel.list().clone()) {
                        bytes += e.text_content().len();
                    }
                }
            }
            let extract = t1.elapsed();

            // Pass 3: same extraction on warm handles (string frames already inflated),
            // isolating the per-sweep zstd inflation cost.
            let docs: Vec<OwnedDoc> = (0..n_docs)
                .map(|pos| OwnedDoc::new(arc.clone(), pos).expect("doc"))
                .collect();
            let warm_pass = |docs: &[OwnedDoc]| {
                let mut bytes = 0usize;
                for doc in docs {
                    let root = HtmlElement::new(doc, NodeIndex::ROOT);
                    if is_attr {
                        for e in root.select(sel.list().clone()) {
                            bytes += e.get_attribute("href").map_or(0, |s| s.len());
                        }
                    } else {
                        for e in root.select(sel.list().clone()) {
                            bytes += e.text_content().len();
                        }
                    }
                }
                bytes
            };
            warm_pass(&docs); // warm the inflate caches
            let t2 = Instant::now();
            warm_pass(&docs);
            let warm = t2.elapsed();
            println!(
                "{src:26} rep{rep} matches={n:7} select={:8.1?} select+extract={:8.1?} warm-extract={:8.1?} bytes={bytes}",
                select, extract, warm
            );
        }
    }
}
