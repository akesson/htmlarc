//! zstd framing-granularity probe — gates `BUNDLE_CAP` sizing (ADR 0004) and the Lane A/B
//! storage work (ADR 0001/0002). An `#[ignore]`d test, not wired into the CLI; kept to confirm
//! the framing numbers on more segments / a ZIM input and to re-measure once Lane B lands.
//!
//! Answers two questions on real data before Lane B exists:
//!   1. How much does a smaller bundle (1k-doc frames) cost vs one frame per 10k bundle?
//!   2. Does a per-bundle *trained dictionary* let per-doc compression (cheap random access)
//!      approach the per-bundle frame ratio? — i.e. is "first docs train, rest use it" worth it?
//!
//! Measures whichever lane `SPIKE_LANE` selects (`b` = text/content-attr values [default],
//! `a` = class/id/searched/ext symbol names), extracted via the `stats` counting pass
//! (`count_doc`). That is a slight under-statement of the eventual on-disk lanes (the production
//! form drops markup it re-derives), but the *relative* framing comparison is unaffected.
//!
//! Run (WARC or ZIM input; uses the tolerant counting tokenizer, not the full parser):
//!   SPIKE_INPUT=…/cc_000.warc.gz   # or a .zim — format inferred from the path
//!   SPIKE_LIMIT=10000 SPIKE_LEVEL=19 SPIKE_LANE=b \
//!     cargo test -p htmlarc-convert --release framing_spike -- --ignored --nocapture

use std::path::Path;

use super::counter::count_doc;
use crate::source::{DocSink, MetaRow, open_source};

const SUB: usize = 1000; // sub-frame size for the "smaller bundle" variant
const DICT_MAX: usize = 112_640; // 110 KiB trained-dict cap

fn level() -> i32 {
    std::env::var("SPIKE_LEVEL")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(19)
}

/// Collects each document's bytes for the selected lane (`b` = text/content-attr values,
/// `a` = class/id/searched/ext symbol names) for one bundle.
struct Collect {
    lane_b: bool,
    docs: Vec<Vec<u8>>,
}
impl DocSink for Collect {
    fn accept(&mut self, _key: &str, html: &str, _meta: Option<MetaRow>) {
        let s = count_doc(html, self.lane_b);
        let bytes = if self.lane_b {
            s.lane_b
        } else {
            s.lane_a.join("\n").into_bytes()
        };
        if !bytes.is_empty() {
            self.docs.push(bytes);
        }
    }
}

fn zc(data: &[u8], lvl: i32) -> usize {
    zstd::bulk::compress(data, lvl).unwrap().len()
}

struct Totals {
    docs: usize,
    raw: usize,
    one_frame: usize, // (A) one zstd frame over the whole sample — best ratio, coarse reads
    sub_frames: usize, // (B) one frame per `SUB` docs — the smaller-bundle proxy
    per_doc: usize,   // (C) one frame per doc — full random access, no cross-doc sharing
    per_doc_dict: usize, // (D) per doc, against a trained dict (excl. dict bytes)
    dict_bytes: usize, // (D) the dict that must be stored once per bundle
}

fn measure_bundle(docs: &[Vec<u8>], lvl: i32) -> Totals {
    let raw: usize = docs.iter().map(|d| d.len()).sum();

    // (A) whole bundle as one frame.
    let mut all = Vec::with_capacity(raw);
    for d in docs {
        all.extend_from_slice(d);
    }
    let one_frame = zc(&all, lvl);

    // (B) one frame per 1k docs.
    let mut sub_frames = 0usize;
    for chunk in docs.chunks(SUB) {
        let mut buf = Vec::new();
        for d in chunk {
            buf.extend_from_slice(d);
        }
        sub_frames += zc(&buf, lvl);
    }

    // (C) one frame per doc.
    let per_doc: usize = docs.iter().map(|d| zc(d, lvl)).sum();

    // (D) per doc, against a dict trained on this bundle's docs. NOTE: trained on the same docs
    // it then compresses, so this slightly OVER-states the dict (real use trains on a sample /
    // held-out set). Good enough to see whether the dict closes the per-doc→per-bundle gap.
    let (per_doc_dict, dict_bytes) = match zstd::dict::from_samples(docs, DICT_MAX) {
        Ok(dict) if !dict.is_empty() => {
            let mut comp = zstd::bulk::Compressor::with_dictionary(lvl, &dict).unwrap();
            let sum: usize = docs.iter().map(|d| comp.compress(d).unwrap().len()).sum();
            (sum, dict.len())
        }
        _ => (per_doc, 0),
    };

    Totals {
        docs: docs.len(),
        raw,
        one_frame,
        sub_frames,
        per_doc,
        per_doc_dict,
        dict_bytes,
    }
}

fn mib(n: usize) -> f64 {
    n as f64 / (1024.0 * 1024.0)
}

#[test]
#[ignore = "framing probe; set SPIKE_INPUT to a real .warc.gz or .zim"]
fn framing_spike() {
    let path = std::env::var("SPIKE_INPUT")
        .or_else(|_| std::env::var("SPIKE_WARC"))
        .expect("set SPIKE_INPUT=/path/to/cc.warc.gz or a .zim");
    let limit = std::env::var("SPIKE_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000usize);
    let lvl = level();
    let lane_b = std::env::var("SPIKE_LANE")
        .map(|s| s != "a")
        .unwrap_or(true);
    let lane = if lane_b { "B (text)" } else { "A (symbols)" };

    // Treat the whole driven sample (up to `limit`) as ONE logical bundle and compare framings
    // within it — deliberately independent of the source's `BUNDLE_CAP` run chunking, so the
    // 1k-vs-Nk comparison means the same thing whatever BUNDLE_CAP currently is. (WARC or ZIM:
    // the format is inferred from the path.)
    let src = open_source(Path::new(&path), None, None, Some(limit)).unwrap();
    let mut docs: Vec<Vec<u8>> = Vec::new();
    for rank in 0..src.run_count() {
        let mut c = Collect {
            lane_b,
            docs: Vec::new(),
        };
        src.drive_run(rank, &mut c);
        docs.append(&mut c.docs);
    }
    eprintln!(
        "framing_spike: lane {lane} — {path}\n  {} docs collected ({} prepared), zstd level {lvl}, sub-frame={SUB}\n",
        docs.len(),
        src.stats().prepared
    );
    let agg = measure_bundle(&docs, lvl);

    let r = |comp: usize| agg.raw as f64 / comp.max(1) as f64;
    let vs_one =
        |comp: usize| 100.0 * (comp as f64 - agg.one_frame as f64) / agg.one_frame.max(1) as f64;
    let dict_total = agg.per_doc_dict + agg.dict_bytes;

    println!(
        "\n=== Lane {lane} framing — {} docs, {:.1} MiB raw, zstd {lvl} ===",
        agg.docs,
        mib(agg.raw)
    );
    println!(
        "{:<34} {:>10} {:>8} {:>12}",
        "strategy", "MiB", "ratio", "vs 1-frame"
    );
    println!("{:-<66}", "");
    println!(
        "{:<34} {:>10.1} {:>7.2}× {:>11}",
        "(A) one frame / bundle",
        mib(agg.one_frame),
        r(agg.one_frame),
        "baseline"
    );
    println!(
        "{:<34} {:>10.1} {:>7.2}× {:>+10.1}%",
        format!("(B) one frame / {SUB} docs"),
        mib(agg.sub_frames),
        r(agg.sub_frames),
        vs_one(agg.sub_frames)
    );
    println!(
        "{:<34} {:>10.1} {:>7.2}× {:>+10.1}%",
        "(C) one frame / doc",
        mib(agg.per_doc),
        r(agg.per_doc),
        vs_one(agg.per_doc)
    );
    println!(
        "{:<34} {:>10.1} {:>7.2}× {:>+10.1}%",
        "(D) per-doc + trained dict",
        mib(dict_total),
        r(dict_total),
        vs_one(dict_total)
    );
    println!(
        "      └ dict overhead: {:.2} MiB (one dict over {} docs)",
        mib(agg.dict_bytes),
        agg.docs
    );
    println!(
        "\nreads: A=whole bundle, B={SUB} docs, C/D=1 doc per decompress.\n\
         key deltas: 1k-vs-1frame {:+.1}%   per-doc-vs-1frame {:+.1}%   dict recovers {:+.1}% of the per-doc penalty",
        vs_one(agg.sub_frames),
        vs_one(agg.per_doc),
        100.0 * (agg.per_doc as f64 - dict_total as f64)
            / (agg.per_doc as f64 - agg.one_frame as f64).max(1.0),
    );
}
