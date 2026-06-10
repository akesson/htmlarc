//! The `export` command plus the small ZIM helpers shared by all three commands.
//!
//! Export is two passes. **Pass 1** (sequential, cheap) scans the ZIM directory and builds an
//! ordered, de-duplicated work list grouped by cluster — only `u32`s and title strings, no
//! decompression — then groups consecutive clusters into **runs** of about [`BUNDLE_CAP`]
//! documents. **Pass 2** (parallel) hands whole runs to a pool of worker threads that share the
//! memory-mapped `&Zim`: one worker owns a run end-to-end, decompressing each of its clusters
//! once (in order) and parsing the HTML blobs into owned [`HtmlEntry`]s off-thread (parsing is
//! the CPU bottleneck). The run is exactly one archive bundle. A coordinator reassembles
//! completed runs in ascending order and seals each as a bundle in the [`ArchiveWriter`], so the
//! bundle layout is cluster-aligned and deterministic regardless of the thread count. A permit
//! semaphore caps how many runs may be in flight, bounding peak memory.
//!
//! One worker owning a whole cluster-aligned run is the foundation for per-bundle string storage:
//! a later step can build that bundle's shared dictionary in-process during this same parse, with
//! no shared mutable state across workers.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Display};
use std::path::Path;
use std::str::from_utf8;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, mpsc};
use std::time::Instant;

use anyhow::{Result, anyhow};
use htmlarc_archive::{ArchiveWriter, BUNDLE_CAP, HtmlEntry};
use htmlarc_dom::prelude::HtmlDoc;
use unicode_normalization::{UnicodeNormalization, is_nfc};
use zim::{MimeType, Namespace, Target, Zim};

use crate::args::Export;

/// One cluster of work: its ZIM cluster index plus the `(blob index, archive key)` of every
/// html article it holds, in directory order.
type ClusterWork = (u32, Vec<(u32, String)>);

/// A run of consecutive clusters that together form one archive bundle (~[`BUNDLE_CAP`] docs).
/// A single worker owns a run: it decompresses and parses every cluster in it, in order.
type Run = Vec<ClusterWork>;

/// Open a ZIM file, turning the crate's terse error into a clear message.
pub(crate) fn open(path: &Path) -> Result<Zim> {
    Zim::new(path).map_err(|e| anyhow!("could not open ZIM {}: {e:?}", path.display()))
}

/// Articles live in the `A` namespace (old scheme) or `C`/UserContent (new scheme),
/// so accept both to stay agnostic to the ZIM's age.
pub(crate) fn is_content(ns: &Namespace) -> bool {
    matches!(ns, Namespace::Articles | Namespace::UserContent)
}

/// True only for `text/html` entries (skips images, css, js, redirects, ...).
pub(crate) fn html_mime(m: &MimeType) -> bool {
    matches!(m, MimeType::Type(s) if s.starts_with("text/html"))
}

/// NFC-normalize a string. ZIM titles and the wordlist are compared in NFC.
pub(crate) fn nfc(s: &str) -> String {
    if is_nfc(s) {
        s.to_string()
    } else {
        s.nfc().collect()
    }
}

/// The archive key for an entry: its NFC title, falling back to the URL slug when the title
/// is empty. Many ZIM HTML resources carry no title but a unique URL; without this fallback
/// they would all collapse into a single empty-keyed entry.
pub(crate) fn key_for(title: &str, url: &str) -> String {
    nfc(if title.is_empty() { url } else { title })
}

/// Parse a wordlist file's contents into an NFC-normalized set (blank lines ignored).
pub(crate) fn parse_wordlist(content: &str) -> HashSet<String> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(nfc)
        .collect()
}

fn load_wordlist(path: &Path) -> Result<HashSet<String>> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow!("could not read wordlist {}: {e}", path.display()))?;
    Ok(parse_wordlist(&content))
}

/// Tally + timing for an export run.
pub(crate) struct Measurements {
    pub exported: u32,
    pub ignored: u32,
    pub failed: u32,
    /// Articles dropped in pass 1 because their key already appeared (first wins).
    pub collapsed: u32,
    start: Instant,
}

impl Measurements {
    fn new() -> Self {
        Self {
            exported: 0,
            ignored: 0,
            failed: 0,
            collapsed: 0,
            start: Instant::now(),
        }
    }
    fn processed(&self) -> u32 {
        self.exported + self.ignored + self.failed
    }
}

impl Display for Measurements {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Processed: {}\nExported: {}\nIgnored: {}\nFailed: {}",
            self.processed(),
            self.exported,
            self.ignored,
            self.failed
        )?;
        let d = self.start.elapsed();
        writeln!(
            f,
            "Time elapsed: {}m {}s",
            d.as_secs() / 60,
            d.as_secs() % 60
        )
    }
}

/// A counting semaphore (std has none): caps how many runs are in flight so peak memory is
/// bounded by `permits` runs' worth (~`permits` × `BUNDLE_CAP`) of parsed documents rather than
/// the whole corpus.
struct Semaphore {
    count: Mutex<usize>,
    cv: Condvar,
}

impl Semaphore {
    fn new(n: usize) -> Self {
        Self {
            count: Mutex::new(n),
            cv: Condvar::new(),
        }
    }
    fn acquire(&self) {
        let mut n = self.count.lock().unwrap();
        while *n == 0 {
            n = self.cv.wait(n).unwrap();
        }
        *n -= 1;
    }
    fn release(&self) {
        *self.count.lock().unwrap() += 1;
        self.cv.notify_one();
    }
}

/// One run's parse result (one bundle), sent from a worker to the coordinator. `rank` is the
/// run's position in the ordered work list, used to reassemble bundles in canonical order.
struct BundleResult {
    rank: usize,
    entries: Vec<HtmlEntry>,
    failed: u32,
}

/// Decompress one cluster (once) and parse every html blob in it, appending the built
/// [`HtmlEntry`]s to `entries` and counting unreadable/unparsable blobs into `failed`.
fn parse_cluster_into(
    zim: &Zim,
    cluster_idx: u32,
    blobs: &[(u32, String)],
    entries: &mut Vec<HtmlEntry>,
    failed: &mut u32,
) {
    let cluster = match zim.get_cluster(cluster_idx) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cluster {cluster_idx} read failed: {e:?}");
            *failed += blobs.len() as u32;
            return;
        }
    };

    for (blob, key) in blobs {
        let bytes = match cluster.get_blob(*blob) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("blob read failed for '{key}': {e:?}");
                *failed += 1;
                continue;
            }
        };
        let html = match from_utf8(bytes.as_ref()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("'{key}' is not valid utf-8: {e}");
                *failed += 1;
                continue;
            }
        };
        match HtmlDoc::parse(html) {
            // Build the HtmlEntry here (optimal node width + checksum) so all the heavy
            // per-document work happens on the worker thread.
            Ok(doc) => entries.push(HtmlEntry::new(key.clone(), doc)),
            Err(e) => {
                eprintln!("parse failed for '{key}': {e}");
                *failed += 1;
            }
        }
    }
}

/// Parse a whole run — every cluster in it, in order — into one bundle's worth of [`HtmlEntry`]s.
/// Each cluster is decompressed exactly once and only by this worker.
fn parse_run(zim: &Zim, rank: usize, run: &Run) -> BundleResult {
    let docs: usize = run.iter().map(|(_, blobs)| blobs.len()).sum();
    let mut entries = Vec::with_capacity(docs);
    let mut failed = 0u32;
    for (cluster_idx, blobs) in run {
        parse_cluster_into(zim, *cluster_idx, blobs, &mut entries, &mut failed);
    }
    BundleResult {
        rank,
        entries,
        failed,
    }
}

/// Group the ascending, de-duplicated cluster work list into runs of about [`BUNDLE_CAP`]
/// documents. Clusters are never split (one worker decompresses a cluster once), so a run is
/// sealed as soon as it reaches `BUNDLE_CAP` — making it `BUNDLE_CAP` rounded up to a cluster
/// boundary — and the final run takes the remainder. An oversized single cluster forms its own
/// run. The grouping depends only on the work list, so the resulting bundles are deterministic.
pub(crate) fn group_into_runs(clusters: Vec<ClusterWork>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut current: Run = Vec::new();
    let mut current_docs = 0usize;
    for cluster in clusters {
        current_docs += cluster.1.len();
        current.push(cluster);
        if current_docs >= BUNDLE_CAP {
            runs.push(std::mem::take(&mut current));
            current_docs = 0;
        }
    }
    if !current.is_empty() {
        runs.push(current);
    }
    runs
}

/// Pass 1: scan the directory and build the ordered, de-duplicated per-cluster work list. The
/// returned clusters are in ascending cluster-index order (the canonical document order), so the
/// coordinator can stream them out deterministically.
fn build_work_list(
    zim: &Zim,
    wordlist: Option<&HashSet<String>>,
    limit: Option<usize>,
) -> (Vec<ClusterWork>, Measurements) {
    let mut counts = Measurements::new();
    let mut groups: BTreeMap<u32, Vec<(u32, String)>> = BTreeMap::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut accepted = 0usize;

    for entry in zim.iterate_by_urls() {
        if let Some(lim) = limit
            && accepted >= lim
        {
            break; // sampled enough — stop scanning the directory
        }
        if !is_content(&entry.namespace) {
            continue; // metadata / fulltext-index / categories: not articles, don't count
        }
        match &entry.target {
            Some(Target::Cluster(c, b)) if html_mime(&entry.mime_type) => {
                let key = key_for(&entry.title, &entry.url);
                if let Some(set) = wordlist
                    && !set.contains(&key)
                {
                    counts.ignored += 1;
                    continue;
                }
                // Dedup here (first wins, in directory order) so workers never produce a
                // duplicate key and the bundle layout is fully deterministic.
                if !seen.insert(key.clone()) {
                    counts.collapsed += 1;
                    continue;
                }
                groups.entry(*c).or_default().push((*b, key));
                accepted += 1;
            }
            // redirects, non-html content (images/css/js), link-targets, deleted entries:
            _ => counts.ignored += 1,
        }
    }

    (groups.into_iter().collect(), counts)
}

pub(crate) fn run(args: Export) -> Result<()> {
    let Export {
        file,
        output,
        list,
        limit,
    } = args;
    let zim = open(&file)?;
    let wordlist = match &list {
        Some(path) => Some(load_wordlist(path)?),
        None => None,
    };

    // Pass 1: ordered, de-duplicated work list (cheap — titles + u32s only), then group
    // consecutive clusters into cluster-aligned runs that each become one archive bundle.
    let (clusters, mut counts) = build_work_list(&zim, wordlist.as_ref(), limit);
    let runs = group_into_runs(clusters);

    // Pass 2: parse runs in parallel, seal each completed run as a bundle in rank order.
    let mut writer = ArchiveWriter::create(&output)?;
    // Worker count defaults to the available parallelism; `ZIM2HTMLARC_THREADS` overrides it
    // (handy for benchmarking and for asserting the output is independent of the thread count).
    let thread_count = std::env::var("ZIM2HTMLARC_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |p| p.get()));
    // Cap runs in flight (grabbed but not yet written). >= thread_count keeps every worker busy;
    // the extra slack absorbs out-of-order completion so the reorder buffer stays bounded. Each
    // run carries ~BUNDLE_CAP parsed docs, so this also sets the peak-memory ceiling.
    let in_flight_cap = (thread_count * 2).max(thread_count + 4);

    let next_rank = AtomicUsize::new(0);
    let permits = Semaphore::new(in_flight_cap);
    let (tx, rx) = mpsc::channel::<BundleResult>();

    let mut write_err: Option<anyhow::Error> = None;

    std::thread::scope(|scope| {
        for _ in 0..thread_count {
            let tx = tx.clone();
            let next_rank = &next_rank;
            let permits = &permits;
            let zim = &zim;
            let runs = &runs;
            scope.spawn(move || {
                loop {
                    // Acquire a permit *before* grabbing work so at most `in_flight_cap` runs
                    // are resident at once.
                    permits.acquire();
                    let rank = next_rank.fetch_add(1, Ordering::SeqCst);
                    if rank >= runs.len() {
                        // Out of work: hand the permit back so other workers don't starve.
                        permits.release();
                        break;
                    }
                    let result = parse_run(zim, rank, &runs[rank]);
                    // Unbounded send never blocks; the permit cap already bounds in-flight work,
                    // so the channel holds at most `in_flight_cap` results.
                    if tx.send(result).is_err() {
                        permits.release();
                        break;
                    }
                }
            });
        }
        // Drop our own sender so `rx` closes once every worker has finished.
        drop(tx);

        // Coordinator: reassemble runs in ascending rank order and seal each as one bundle.
        let mut buffer: BTreeMap<usize, BundleResult> = BTreeMap::new();
        let mut next_emit = 0usize;
        for result in rx {
            buffer.insert(result.rank, result);
            while let Some(result) = buffer.remove(&next_emit) {
                if write_err.is_none() {
                    for entry in &result.entries {
                        if let Err(e) = writer.push_entry(entry) {
                            write_err = Some(e.into());
                            break;
                        }
                        counts.exported += 1;
                    }
                    // Seal the run as its own bundle (a no-op if it parsed to zero documents), so
                    // on-disk bundles are exactly the cluster-aligned runs from pass 1.
                    if write_err.is_none() {
                        writer.seal_bundle();
                    }
                }
                counts.failed += result.failed;
                next_emit += 1;
                // Releasing a permit only as a run is written keeps memory bounded; always
                // release (even after an error) so workers drain and the scope can join.
                permits.release();
            }
        }
    });

    if let Some(e) = write_err {
        return Err(e);
    }

    let stored = writer.doc_count() as u32;
    writer.finish()?;

    println!("{counts}");
    if counts.collapsed > 0 {
        println!(
            "Note: {} article(s) shared a key and were merged ({stored} unique in archive).",
            counts.collapsed
        );
    }
    Ok(())
}
