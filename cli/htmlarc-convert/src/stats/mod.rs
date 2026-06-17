//! The `stats` command: probe per-document and per-bundle cardinalities to gate ADR 0002's
//! reference-space constants. Each run is counted in parallel; the coordinator merges the
//! per-metric histograms and collects each run's shared-dictionary simulation. With
//! `--compress`, it also measures per-bundle zstd of Lane A vs Lane B.

mod counter;
#[cfg(test)]
mod framing_spike;

use std::collections::HashMap;
use std::io::Write;

use anyhow::Result;
use htmlarc_dom::dom::TopologyReport;
use htmlarc_dom::prelude::HtmlDoc;

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

// Shared-dictionary sizes to sweep.
const SHARED_K: [usize; 5] = [256, 1024, 4096, 16384, 65535];

// zstd level for the (optional) Lane A / Lane B compression measurement — matches the
// reference level in ADR 0001.
const ZSTD_LEVEL: i32 = 19;

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

    /// Number of documents whose value exceeds `2^exp` (i.e. would not fit a field sized to
    /// hold values `≤ 2^exp`). Coarse to the log bucket.
    fn over_pow2(&self, exp: usize) -> u64 {
        self.buckets[(exp + 1).min(self.buckets.len())..]
            .iter()
            .sum()
    }

    fn report_line(&self) -> String {
        let over = if self.threshold == NO_CAP {
            "—".to_string()
        } else {
            format!("{} (>{})", self.over, self.threshold)
        };
        format!(
            "{:<15} max={:<9} p50={:<7} p99={:<8} p99.9={:<8} p99.99={:<8} p99.999={:<8} over_cap={}",
            self.label,
            self.max,
            self.percentile(0.50),
            self.percentile(0.99),
            self.percentile(0.999),
            self.percentile(0.9999),
            self.percentile(0.99999),
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

    fn by_label(&self, label: &str) -> &Hist {
        self.hists.iter().find(|h| h.label == label).unwrap()
    }
}

/// Per-bundle zstd measurement of the two lanes.
#[derive(Clone, Copy, Default)]
struct Compression {
    a_raw: u64, // Lane A bytes stored per-doc (no cross-doc dedup)
    a_z: u64,   // …compressed
    b_raw: u64, // Lane B bytes (text + content-attr values)
    b_z: u64,   // …compressed
}

/// A byte-counting sink — the encoders write into it; we only want the compressed length.
#[derive(Default)]
struct CountWriter(u64);

impl Write for CountWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len() as u64;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Streaming zstd encoders for one bundle's two lanes, plus raw byte tallies.
struct CompressAcc {
    a: zstd::stream::write::Encoder<'static, CountWriter>,
    b: zstd::stream::write::Encoder<'static, CountWriter>,
    a_raw: u64,
    b_raw: u64,
}

impl CompressAcc {
    fn new() -> Self {
        Self {
            a: zstd::stream::write::Encoder::new(CountWriter::default(), ZSTD_LEVEL).unwrap(),
            b: zstd::stream::write::Encoder::new(CountWriter::default(), ZSTD_LEVEL).unwrap(),
            a_raw: 0,
            b_raw: 0,
        }
    }

    fn feed_a(&mut self, s: &str) {
        self.a.write_all(s.as_bytes()).unwrap();
        self.a_raw += s.len() as u64;
    }

    fn feed_b(&mut self, bytes: &[u8]) {
        self.b.write_all(bytes).unwrap();
        self.b_raw += bytes.len() as u64;
    }

    fn finish(self) -> Compression {
        let a_z = self.a.finish().unwrap().0;
        let b_z = self.b.finish().unwrap().0;
        Compression {
            a_raw: self.a_raw,
            a_z,
            b_raw: self.b_raw,
            b_z,
        }
    }
}

/// ADR 0002 PR 6 — does the node record carry one width or two? Node-*link* slots (parent/
/// sibling/child) must escalate to u24 once a document's node count exceeds 65,535; the
/// class/attr slots (RunVec arena offsets) escalate independently once `list_entries` exceeds
/// 65,535. This tallies the *joint* crossing and the resulting topology bytes under the two
/// candidate layouts:
///   * single-width: one width per record; u24 ⇒ links AND refs are 3 B  → 15 / 22 B per node
///   * mixed-width:  link- and ref-width chosen independently             → 15 / 17 / 20 / 22 B
///
/// The two differ only on documents where exactly one axis overflows u16. Thresholding the ref
/// axis on `list_entries` (ignoring the per-run arena terminators, which push the real arena
/// over u16 slightly *earlier*) is deliberately generous to mixed-width.
#[derive(Clone, Copy, Default)]
struct WidthImpact {
    /// Document counts by cell `link24 | (ref24 << 1)`: [both-fit, link-only, ref-only, both].
    docs: [u64; 4],
    /// Summed node *count* per cell (× bytes-per-node under a policy = that cell's topology).
    nodes: [u64; 4],
}

impl WidthImpact {
    fn record(&mut self, nodes: u32, list_entries: u32) {
        let link24 = nodes > 65_535;
        let ref24 = list_entries > 65_535;
        let cell = link24 as usize | ((ref24 as usize) << 1);
        self.docs[cell] += 1;
        self.nodes[cell] += nodes as u64;
    }

    fn merge(&mut self, o: &WidthImpact) {
        for i in 0..4 {
            self.docs[i] += o.docs[i];
            self.nodes[i] += o.nodes[i];
        }
    }
}

/// ADR 0002 topology-packing probe (`--topology`) — post-PR-5, topology is ~62 % of the
/// compressed general-web archive and the next size lever. Unlike the cardinality counters
/// this needs the *real* parser (a tolerant token pass can't build a tree), so it runs the
/// exact production path (`parse → into_optimal_width`) and aggregates each document's
/// [`TopologyReport`]. It also measures the blob after a document-order [`rebuild`], to
/// separate the "reorder + drop dead slots" win from the "delta-encode the links" win.
/// Documents that fail to parse are skipped (as in production) and tallied.
#[derive(Default)]
struct TopoAcc {
    parsed: u64,
    failed: u64,
    /// The blob as stored today (`parse → into_optimal_width`).
    serialized: TopologyReport,
    /// The blob after a document-order `rebuild()` (dead slots dropped, indices renumbered).
    rebuilt: TopologyReport,
}

impl TopoAcc {
    fn record(&mut self, html: &str) {
        match HtmlDoc::parse(html) {
            Ok(doc) => {
                let dom = doc.dom();
                self.serialized
                    .merge(&dom.clone().into_optimal_width().topology_report());
                self.rebuilt
                    .merge(&dom.rebuild().into_optimal_width().topology_report());
                self.parsed += 1;
            }
            Err(_) => self.failed += 1,
        }
    }

    fn merge(&mut self, o: &TopoAcc) {
        self.parsed += o.parsed;
        self.failed += o.failed;
        self.serialized.merge(&o.serialized);
        self.rebuilt.merge(&o.rebuilt);
    }
}

/// One bundle's results: shared-dictionary coverage, node total, and (optionally) compression.
struct BundleDict {
    at_k: [(f64, u64); SHARED_K.len()],
    node_sum: u64,
    width: WidthImpact,
    compression: Option<Compression>,
    topo: Option<TopoAcc>,
}

/// Accumulates one run's documents: histograms, the Lane A doc-frequency map, node total, and
/// (optionally) the per-bundle compression encoders.
struct StatsSink {
    hists: HistSet,
    freq: HashMap<String, (u32, u32)>, // symbol -> (doc frequency, byte length)
    node_sum: u64,
    width: WidthImpact,
    compress: Option<CompressAcc>,
    topo: Option<TopoAcc>,
}

impl StatsSink {
    fn new(compress: bool, topology: bool) -> Self {
        Self {
            hists: HistSet::new(),
            freq: HashMap::new(),
            node_sum: 0,
            width: WidthImpact::default(),
            compress: compress.then(CompressAcc::new),
            topo: topology.then(TopoAcc::default),
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
        let compression = self.compress.map(CompressAcc::finish);
        (
            self.hists,
            BundleDict {
                at_k,
                node_sum: self.node_sum,
                width: self.width,
                compression,
                topo: self.topo,
            },
        )
    }
}

impl DocSink for StatsSink {
    fn accept(&mut self, key: &str, html: &str) {
        let stats = count_doc(html, self.compress.is_some());
        self.hists.record(&stats, key);
        self.node_sum += stats.nodes as u64;
        self.width.record(stats.nodes, stats.list_entries);
        for token in &stats.lane_a {
            if let Some(e) = self.freq.get_mut(token) {
                e.0 += 1;
            } else {
                self.freq.insert(token.clone(), (1, token.len() as u32));
            }
        }
        if let Some(c) = &mut self.compress {
            for token in &stats.lane_a {
                c.feed_a(token);
            }
            c.feed_b(&stats.lane_b);
        }
        if let Some(t) = &mut self.topo {
            t.record(html);
        }
    }
}

/// Count every run of `source` in parallel, merging into the running totals.
fn accumulate(
    source: &dyn Source,
    compress: bool,
    topology: bool,
    global: &mut HistSet,
    bundles: &mut Vec<BundleDict>,
) {
    drive_runs_parallel(
        source.run_count(),
        |rank| {
            let mut sink = StatsSink::new(compress, topology);
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
        compress,
        topology,
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
            accumulate(&source, compress, topology, &mut global, &mut bundles);
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
        accumulate(
            source.as_ref(),
            compress,
            topology,
            &mut global,
            &mut bundles,
        );
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

    // What index width covers what fraction of docs (for sizing the list pointer / sym refs).
    let total = prepared.max(1) as f64;
    println!("\nIndex-width coverage (docs NOT fitting an N-bit field):");
    println!(
        "  {:<15} >2^15(32768)   >2^16(65536)   >2^17(131072)",
        "metric"
    );
    for label in ["list_entries", "sym_union", "distinct_pairs"] {
        let h = global.by_label(label);
        let pct = |n: u64| 100.0 * n as f64 / total;
        println!(
            "  {:<15} {:>7} ({:.4}%)  {:>6} ({:.4}%)  {:>5} ({:.4}%)",
            label,
            h.over_pow2(15),
            pct(h.over_pow2(15)),
            h.over_pow2(16),
            pct(h.over_pow2(16)),
            h.over_pow2(17),
            pct(h.over_pow2(17)),
        );
    }

    // ADR 0002 PR 6: node-record width policy — mixed (independent link/ref width) vs single.
    let mut w = WidthImpact::default();
    for b in bundles {
        w.merge(&b.width);
    }
    let docs_total: u64 = w.docs.iter().sum();
    if docs_total > 0 {
        let nb = &w.nodes; // node COUNT per cell: [both-fit, link-only, ref-only, both]
        let single = nb[0] * 15 + (nb[1] + nb[2] + nb[3]) * 22;
        let mixed = nb[0] * 15 + nb[1] * 20 + nb[2] * 17 + nb[3] * 22;
        let saving = single - mixed; // = nb[1]*2 + nb[2]*5
        let pct = |n: u64| 100.0 * n as f64 / docs_total as f64;
        println!("\nNode-record width policy (ADR 0002 PR 6) — joint u16 crossing (>65,535):");
        println!(
            "  docs: both-fit-u16={} link-u24-only={} ref-u24-only={} both-u24={}",
            w.docs[0], w.docs[1], w.docs[2], w.docs[3]
        );
        println!(
            "  link-u24 needed (nodes>65535):       {} ({:.5}%)",
            w.docs[1] + w.docs[3],
            pct(w.docs[1] + w.docs[3])
        );
        println!(
            "  ref-u24  needed (list_entries>65535): {} ({:.5}%)   [today these docs are skipped]",
            w.docs[2] + w.docs[3],
            pct(w.docs[2] + w.docs[3])
        );
        println!(
            "  topology bytes  single-width={}  mixed-width={}",
            human_bytes(single),
            human_bytes(mixed)
        );
        println!(
            "  → mixed-width saves {} = {:.4}% of single-width topology",
            human_bytes(saving),
            100.0 * saving as f64 / single.max(1) as f64
        );
        println!(
            "    (saving = {} link-only nodes ×2 B + {} ref-only nodes ×5 B)",
            nb[1], nb[2]
        );
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
            "  K={:<6} coverage min={:.1}% mean={:.1}% max={:.1}%   bytes saved (sum)={}",
            k,
            min * 100.0,
            mean * 100.0,
            max * 100.0,
            human_bytes(bytes),
        );
    }

    print_compression(bundles);
    print_topology(bundles);
}

/// ADR 0002 topology-packing probe report: today's on-disk topology vs. the
/// document-order-rebuilt and link-packed alternatives, with the per-link delta distribution
/// that gates which packing is worth its hot-path cost.
fn print_topology(bundles: &[BundleDict]) {
    let mut t = TopoAcc::default();
    for b in bundles {
        if let Some(bt) = &b.topo {
            t.merge(bt);
        }
    }
    if t.parsed == 0 {
        return;
    }

    let pct = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
    println!("\nTopology packing probe (ADR 0002 — node links are 10 of 15 B/node at u16):");
    println!(
        "  Parsed {} docs ({} failed to parse, skipped).",
        t.parsed, t.failed
    );
    print_topo_blob(
        "as serialized today (parse → into_optimal_width)",
        &t.serialized,
    );
    print_topo_blob("after document-order rebuild()", &t.rebuilt);

    // Headline: today's on-disk topology against (a) a rebuild alone — drops dead slots, no
    // format change — and (b) rebuild + link packing. The packed estimate keeps every non-link
    // byte (tag, class/attr refs, text overlays), swaps the fixed link slots for the varint +
    // implicit-first-child encoding, and adds a 1-byte/node link-presence mask.
    let today = t.serialized.record_bytes;
    let rebuilt_only = t.rebuilt.record_bytes;
    let packed = packed_estimate(&t.rebuilt);
    println!("\n  Headline (vs {} on disk today):", human_bytes(today));
    println!(
        "    rebuild alone (drop dead slots, no format change): {} (−{:.1}%)",
        human_bytes(rebuilt_only),
        pct(today.saturating_sub(rebuilt_only), today),
    );
    println!(
        "    rebuild + varint links + implicit first-child + 1 B/node mask: {} (−{:.1}%)",
        human_bytes(packed),
        pct(today.saturating_sub(packed), today),
    );
    println!(
        "    NOTE: varint links are a SIZE ceiling — variable-width defeats the single-load\n    \
         traversal hot path (the NodeWidth +65% lesson). The hot-path-safe subset is dropping\n    \
         dead slots + implicit first-child (fixed width); the varint figure bounds the rest."
    );
}

/// Bytes of a link-packed blob: keep the non-link bytes, replace fixed link slots with the
/// varint + implicit-first-child encoding, add a conservative 1-byte/node link-presence mask.
fn packed_estimate(r: &TopologyReport) -> u64 {
    (r.record_bytes - r.link_bytes_fixed) + r.link_bytes_varint_implicit + r.nodes
}

fn print_topo_blob(label: &str, r: &TopologyReport) {
    if r.nodes == 0 {
        return;
    }
    let pct = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
    println!("\n  --- {label} ---");
    println!(
        "    nodes {}  (elements {}, strings {}, dead {} = {:.2}%)",
        r.nodes,
        r.elements,
        r.strings,
        r.dead,
        pct(r.dead, r.nodes),
    );
    println!(
        "    topology blob {}  (links {} = {:.0}% of it)",
        human_bytes(r.record_bytes),
        human_bytes(r.link_bytes_fixed),
        pct(r.link_bytes_fixed, r.record_bytes),
    );
    println!(
        "    link bytes:  fixed {} → varint {} (−{:.0}%) → +implicit-first-child {} (−{:.0}%)",
        human_bytes(r.link_bytes_fixed),
        human_bytes(r.link_bytes_varint),
        pct(
            r.link_bytes_fixed.saturating_sub(r.link_bytes_varint),
            r.link_bytes_fixed
        ),
        human_bytes(r.link_bytes_varint_implicit),
        pct(
            r.link_bytes_fixed
                .saturating_sub(r.link_bytes_varint_implicit),
            r.link_bytes_fixed
        ),
    );
    // Per-link delta width as % of PRESENT links; the absent (None) share is shown separately.
    const LINKS: [&str; 5] = ["parent", "prev  ", "next  ", "first ", "last  "];
    println!("    per-link delta width (% of present links | none = % absent):");
    for (li, name) in LINKS.iter().enumerate() {
        let row = &r.delta_hist[li];
        let present: u64 = row[1..].iter().sum();
        let all: u64 = row.iter().sum();
        let p = |n: u64| pct(n, present);
        println!(
            "      {name}  1B {:>5.1}  2B {:>5.1}  3B {:>5.1}  4B+ {:>5.1}   | none {:>5.1}",
            p(row[1]),
            p(row[2]),
            p(row[3]),
            p(row[4] + row[5]),
            pct(row[0], all),
        );
    }
    println!(
        "    invariants: first_child==self+1 {:.1}% of elements | next==self+1 {:.1}% | parent==self-1 {:.1}%",
        pct(r.first_is_self_plus1, r.elements),
        pct(r.next_is_self_plus1, r.nodes),
        pct(r.parent_is_self_minus1, r.nodes),
    );
}

fn print_compression(bundles: &[BundleDict]) {
    let comps: Vec<Compression> = bundles.iter().filter_map(|b| b.compression).collect();
    if comps.is_empty() {
        return;
    }
    let a_perdoc: u64 = comps.iter().map(|c| c.a_raw).sum(); // Lane A raw, per-doc (no dict)
    let a_z: u64 = comps.iter().map(|c| c.a_z).sum(); // Lane A compressed (zstd dedups cross-doc)
    let b_raw: u64 = comps.iter().map(|c| c.b_raw).sum();
    let b_z: u64 = comps.iter().map(|c| c.b_z).sum();
    let node_sum: u64 = bundles.iter().map(|b| b.node_sum).sum();
    // Topology est: node count × the post-ADR-0002 u16 record (15 B). Constant across the
    // Lane A storage choices below, so it does not affect the head-to-head comparison.
    let topo = node_sum * 15;

    let ratio = |raw: u64, z: u64| if z == 0 { 0.0 } else { raw as f64 / z as f64 };
    let pct = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;

    println!("\nCompression (per-bundle zstd level {ZSTD_LEVEL}):");
    println!(
        "  Lane B (text + content-attr values)   raw={} → zstd={} ({:.1}×)",
        human_bytes(b_raw),
        human_bytes(b_z),
        ratio(b_raw, b_z)
    );
    println!(
        "  Lane A (class/id/searched/ext names)  per-doc raw={}, zstd={} ({:.1}×)",
        human_bytes(a_perdoc),
        human_bytes(a_z),
        ratio(a_perdoc, a_z)
    );
    println!(
        "  Topology est (nodes × 15 B)           {}",
        human_bytes(topo)
    );

    // Three ways to store Lane A; Lane B (zstd) and topology are the same in each, so the
    // archive differences are entirely the Lane A choice.
    println!("\n  Lane A stored size, three ways (Lane B zstd + topology are constant):");
    println!(
        "    (1) raw, per-doc (no shared dict):    {}",
        human_bytes(a_perdoc)
    );
    for (i, k) in SHARED_K.iter().enumerate() {
        let saving: u64 = bundles.iter().map(|b| b.at_k[i].1).sum();
        let a_stored = a_perdoc.saturating_sub(saving);
        println!(
            "    (2) raw + shared dict @K={:<6}        {:>10}   (dict saves {} vs (1))",
            k,
            human_bytes(a_stored),
            human_bytes(saving)
        );
    }
    println!(
        "    (3) zstd (no raw lane, no index):     {:>10}",
        human_bytes(a_z)
    );

    // Verdict, comparing the design's bounded shared dict (K=4096) against compressing.
    let k_idx = SHARED_K.iter().position(|&k| k == 4096).unwrap_or(0);
    let saving_design: u64 = bundles.iter().map(|b| b.at_k[k_idx].1).sum();
    let a_stored_design = a_perdoc.saturating_sub(saving_design);
    let arch_dict = a_stored_design + b_z + topo;
    let arch_zstd = a_z + b_z + topo;
    println!(
        "\n  At the design K={}: shared dict saves {} = {:.1}% of the dict-archive ({}).",
        SHARED_K[k_idx],
        human_bytes(saving_design),
        pct(saving_design, arch_dict),
        human_bytes(arch_dict),
    );
    if a_stored_design <= a_z {
        println!(
            "  Lane A raw+dict ({}) is SMALLER than zstd ({}) by {} — the dict wins on size too.",
            human_bytes(a_stored_design),
            human_bytes(a_z),
            human_bytes(a_z - a_stored_design),
        );
    } else {
        println!(
            "  Lane A raw+dict ({}) is LARGER than zstd ({}) by {} ({:.1}% of archive): on size,\n  \
             compressing Lane A would beat the shared dict — the dict earns its place on query\n  \
             speed (raw + index-comparison + bundle-skip), not bytes.",
            human_bytes(a_stored_design),
            human_bytes(a_z),
            human_bytes(a_stored_design - a_z),
            pct(a_stored_design - a_z, arch_zstd),
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
