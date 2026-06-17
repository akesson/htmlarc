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
//! Run (needs a real WARC; uses the tolerant counting tokenizer, not the full parser):
//!   SPIKE_WARC=~/Developer/akesson/htmlarc/cli/htmlarc-convert/testdata/cc_000.warc.gz \
//!   SPIKE_LIMIT=10000 SPIKE_LEVEL=19 SPIKE_LANE=b \
//!     cargo test -p htmlarc-convert --release framing_spike -- --ignored --nocapture

use std::path::Path;

use super::counter::count_doc;
use crate::source::{DocSink, Source, WarcSource};

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
    fn accept(&mut self, _key: &str, html: &str) {
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

#[derive(Default, Clone, Copy)]
struct Totals {
    docs: usize,
    raw: usize,
    one_frame: usize, // (A) one zstd frame per bundle  — best ratio, no random access
    sub_frames: usize, // (B) one frame per 1k docs       — the BUNDLE_CAP=1000 proxy
    per_doc: usize,   // (C) one frame per doc           — full random access, no sharing
    per_doc_dict: usize, // (D) per doc, against a per-bundle trained dict (excl. dict bytes)
    dict_bytes: usize, // (D) the dict that must be stored once per bundle
}

impl Totals {
    fn add(&mut self, o: &Totals) {
        self.docs += o.docs;
        self.raw += o.raw;
        self.one_frame += o.one_frame;
        self.sub_frames += o.sub_frames;
        self.per_doc += o.per_doc;
        self.per_doc_dict += o.per_doc_dict;
        self.dict_bytes += o.dict_bytes;
    }
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
#[ignore = "throwaway spike; set SPIKE_WARC to a real .warc.gz"]
fn framing_spike() {
    let path = std::env::var("SPIKE_WARC").expect("set SPIKE_WARC=/path/to/cc.warc.gz");
    let limit = std::env::var("SPIKE_LIMIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(10_000usize);
    let lvl = level();
    let lane_b = std::env::var("SPIKE_LANE")
        .map(|s| s != "a")
        .unwrap_or(true);
    let lane = if lane_b { "B (text)" } else { "A (symbols)" };

    let src = WarcSource::open(Path::new(&path), None, Some(limit)).unwrap();
    eprintln!(
        "framing_spike: lane {lane} — {path}\n  {} run(s), {} prepared docs, zstd level {lvl}, sub-frame={SUB}\n",
        src.run_count(),
        src.stats().prepared
    );

    let mut agg = Totals::default();
    for rank in 0..src.run_count() {
        let mut c = Collect {
            lane_b,
            docs: Vec::new(),
        };
        src.drive_run(rank, &mut c);
        let t = measure_bundle(&c.docs, lvl);
        eprintln!(
            "  bundle {rank}: {} docs, lane {lane} raw {:.1} MiB → one-frame {:.1} MiB ({:.2}×)",
            t.docs,
            mib(t.raw),
            mib(t.one_frame),
            t.raw as f64 / t.one_frame.max(1) as f64,
        );
        agg.add(&t);
    }

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
        "      └ dict overhead: {:.2} MiB total ({} bundle(s))",
        mib(agg.dict_bytes),
        src.run_count()
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
