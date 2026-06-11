//! The `stats` command: probe per-document and per-bundle cardinalities to gate ADR 0002's
//! reference-space constants. Each run is counted in parallel; the coordinator merges the
//! per-metric histograms and collects each run's shared-dictionary simulation.

mod counter;

use std::collections::HashMap;

use anyhow::Result;

use crate::args::Stats;
use crate::source::{DocSink, Source, WarcSource, drive_runs_parallel, open_source, warc_files};
use counter::{DocStats, count_doc};

// ADR 0002 ceilings (the values a per-document count must stay under).
const NODE_CAP: u32 = 0x00FF_FFFF; // u24 node sentinel
const DEPTH_CAP: u32 = 256;
const LIST_CAP: u32 = 32_768; // 15-bit list next-pointer
const HEAP_CAP: u32 = 65_535; // u16 string/entry table
const SYM_LOCAL_CAP: u32 = 0xEF00; // 61,184 per-doc Lane A symbols
const EXT_TAG_VOCAB: u32 = 63; // EXT_BASE..255 tag-byte vocabulary
const NO_CAP: u32 = u32::MAX;

// Shared-dictionary sizes to simulate (ADR 0002: 1024 vs 4095).
const SHARED_K: [usize; 2] = [1024, 4095];

/// A log2-bucketed histogram with an exact max (and the document that produced it) and a
/// count of documents over a hard ceiling.
#[derive(Clone)]
struct Hist {
    label: &'static str,
    threshold: u32,
    buckets: [u64; 33],
    count: u64,
    over: u64,
    max: u32,
    max_key: String,
}

impl Hist {
    fn new(label: &'static str, threshold: u32) -> Self {
        Self {
            label,
            threshold,
            buckets: [0; 33],
            count: 0,
            over: 0,
            max: 0,
            max_key: String::new(),
        }
    }

    fn record(&mut self, v: u32, key: &str) {
        self.count += 1;
        let bucket = if v == 0 {
            0
        } else {
            (32 - v.leading_zeros()) as usize
        };
        self.buckets[bucket] += 1;
        if v > self.max {
            self.max = v;
            self.max_key = key.to_string();
        }
        if v > self.threshold {
            self.over += 1;
        }
    }

    fn merge(&mut self, other: &Hist) {
        for (a, b) in self.buckets.iter_mut().zip(other.buckets.iter()) {
            *a += b;
        }
        self.count += other.count;
        self.over += other.over;
        if other.max > self.max {
            self.max = other.max;
            self.max_key = other.max_key.clone();
        }
    }

    /// Upper bound of the bucket the p-th percentile falls in (approximate).
    fn percentile(&self, p: f64) -> u32 {
        if self.count == 0 {
            return 0;
        }
        let target = (p * self.count as f64).ceil() as u64;
        let mut cum = 0u64;
        for (b, &c) in self.buckets.iter().enumerate() {
            cum += c;
            if cum >= target {
                // The bucket's upper bound can exceed the observed max (the max may sit low
                // within its log bucket); clamp so a percentile never reports above it.
                return bucket_upper(b).min(self.max);
            }
        }
        self.max
    }

    fn report_line(&self) -> String {
        let over = if self.threshold == NO_CAP {
            "—".to_string()
        } else {
            format!("{} (>{})", self.over, self.threshold)
        };
        format!(
            "{:<16} max={:<10} p50={:<7} p90={:<8} p99={:<9} p99.9={:<10} over_cap={}",
            self.label,
            self.max,
            self.percentile(0.50),
            self.percentile(0.90),
            self.percentile(0.99),
            self.percentile(0.999),
            over,
        )
    }
}

fn bucket_upper(b: usize) -> u32 {
    if b == 0 {
        0
    } else if b >= 32 {
        u32::MAX
    } else {
        (1u32 << b) - 1
    }
}

/// The seven per-metric histograms.
#[derive(Clone)]
struct HistSet {
    hists: Vec<Hist>,
}

impl HistSet {
    fn new() -> Self {
        Self {
            hists: vec![
                Hist::new("nodes", NODE_CAP),
                Hist::new("max_depth", DEPTH_CAP),
                Hist::new("list_entries", LIST_CAP),
                Hist::new("distinct_pairs", HEAP_CAP),
                Hist::new("ext_tag_names", EXT_TAG_VOCAB),
                Hist::new("ext_attr_names", NO_CAP),
                Hist::new("sym_union", SYM_LOCAL_CAP),
            ],
        }
    }

    fn record(&mut self, s: &DocStats, key: &str) {
        let values = [
            s.nodes,
            s.max_depth,
            s.list_entries,
            s.distinct_pairs,
            s.ext_tag_names,
            s.ext_attr_names,
            s.sym_union,
        ];
        for (h, v) in self.hists.iter_mut().zip(values) {
            h.record(v, key);
        }
    }

    fn merge(&mut self, other: &HistSet) {
        for (a, b) in self.hists.iter_mut().zip(other.hists.iter()) {
            a.merge(b);
        }
    }
}

/// One bundle's shared-dictionary simulation result.
struct BundleDict {
    /// `(coverage_fraction, bytes_saved)` at each of [`SHARED_K`].
    at_k: [(f64, u64); SHARED_K.len()],
}

/// Accumulates one run's documents: histograms plus the run's Lane A document-frequency map.
struct StatsSink {
    hists: HistSet,
    freq: HashMap<String, (u32, u32)>, // symbol -> (doc frequency, byte length)
}

impl StatsSink {
    fn new() -> Self {
        Self {
            hists: HistSet::new(),
            freq: HashMap::new(),
        }
    }

    /// Simulate a shared dictionary over this run: pick the top-K symbols by bytes saved
    /// (`(doc_freq − 1) × len`) and report what fraction of Lane A references they cover.
    fn into_bundle_dict(self) -> (HistSet, BundleDict) {
        let mut entries: Vec<(u32, u32)> = self.freq.into_values().collect();
        let total_refs: u64 = entries.iter().map(|(df, _)| *df as u64).sum();
        entries.sort_unstable_by_key(|(df, len)| {
            std::cmp::Reverse(df.saturating_sub(1) as u64 * *len as u64)
        });

        let mut at_k = [(0.0, 0u64); SHARED_K.len()];
        for (i, k) in SHARED_K.iter().enumerate() {
            let covered: u64 = entries.iter().take(*k).map(|(df, _)| *df as u64).sum();
            let bytes: u64 = entries
                .iter()
                .take(*k)
                .map(|(df, len)| df.saturating_sub(1) as u64 * *len as u64)
                .sum();
            at_k[i] = (covered as f64 / total_refs.max(1) as f64, bytes);
        }
        (self.hists, BundleDict { at_k })
    }
}

impl DocSink for StatsSink {
    fn accept(&mut self, key: &str, html: &str) {
        let stats = count_doc(html);
        self.hists.record(&stats, key);
        for token in &stats.lane_a {
            if let Some(e) = self.freq.get_mut(token) {
                e.0 += 1;
            } else {
                self.freq.insert(token.clone(), (1, token.len() as u32));
            }
        }
    }
}

/// Count every run of `source` in parallel, merging into the running totals.
fn accumulate(source: &dyn Source, global: &mut HistSet, bundles: &mut Vec<BundleDict>) {
    drive_runs_parallel(
        source.run_count(),
        |rank| {
            let mut sink = StatsSink::new();
            source.drive_run(rank, &mut sink);
            sink.into_bundle_dict()
        },
        |(hists, dict)| {
            global.merge(&hists);
            bundles.push(dict);
        },
    );
}

pub(crate) fn run(args: Stats) -> Result<()> {
    let Stats {
        input,
        limit,
        format,
    } = args;

    let mut global = HistSet::new();
    let mut bundles: Vec<BundleDict> = Vec::new();
    let mut counted = 0usize;

    if let Some(files) = warc_files(&input, format.as_deref())? {
        // Stream WARC segments one file at a time — the WARC reader holds a whole file in
        // memory, so processing (then dropping) each in turn keeps memory bounded to a single
        // segment over a corpus of any size.
        for (i, file) in files.iter().enumerate() {
            let remaining = match limit {
                Some(l) if counted >= l => break,
                Some(l) => Some(l - counted),
                None => None,
            };
            let source = WarcSource::open(file, None, remaining)?;
            counted += source.stats().prepared;
            accumulate(&source, &mut global, &mut bundles);
            eprintln!(
                "  [{}/{}] {} — {counted} docs cumulative",
                i + 1,
                files.len(),
                file.display()
            );
        }
    } else {
        let source = open_source(&input, format.as_deref(), None, limit)?;
        counted = source.stats().prepared;
        accumulate(source.as_ref(), &mut global, &mut bundles);
    }

    print_report(counted, &global, &bundles);
    Ok(())
}

fn print_report(prepared: usize, global: &HistSet, bundles: &[BundleDict]) {
    println!("Documents counted: {prepared}");
    println!("Bundles: {}\n", bundles.len());

    println!("Per-document cardinality (log-bucketed percentiles, exact max):");
    for h in &global.hists {
        println!("  {}", h.report_line());
    }
    for h in &global.hists {
        if h.over > 0 && h.threshold != NO_CAP {
            println!(
                "  ! {} doc(s) exceed the {} cap of {} (worst: '{}' = {})",
                h.over, h.label, h.threshold, h.max_key, h.max
            );
        }
    }

    if bundles.is_empty() {
        return;
    }
    println!("\nPer-bundle shared-dictionary simulation (Lane A reference coverage):");
    for (i, k) in SHARED_K.iter().enumerate() {
        let covs: Vec<f64> = bundles.iter().map(|b| b.at_k[i].0).collect();
        let bytes: u64 = bundles.iter().map(|b| b.at_k[i].1).sum();
        let (min, mean, max) = min_mean_max(&covs);
        println!(
            "  K={:<5} coverage min={:.1}% mean={:.1}% max={:.1}%   bytes saved (sum)={}",
            k,
            min * 100.0,
            mean * 100.0,
            max * 100.0,
            human_bytes(bytes),
        );
    }
}

fn min_mean_max(xs: &[f64]) -> (f64, f64, f64) {
    if xs.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let min = xs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = xs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mean = xs.iter().sum::<f64>() / xs.len() as f64;
    (min, mean, max)
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}
