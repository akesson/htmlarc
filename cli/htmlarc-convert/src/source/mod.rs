//! Multi-source document input.
//!
//! ZIM archives, WARC crawl files, and directories of HTML files are all presented as
//! bundle-sized *runs* of documents that every command consumes the same way: a [`Source`]
//! exposes a fixed number of runs and, on demand, drives one run's documents through a
//! [`DocSink`]. `convert` builds [`HtmlEntry`](htmlarc_archive::HtmlEntry)s; `stats` counts
//! cardinalities; `list`/`extract` print keys or one document.
//!
//! Runs are bundle-aligned and processed in parallel by [`drive_runs_parallel`], which
//! preserves the cluster-aligned, rank-ordered, thread-count-independent emit order that the
//! archive format relies on.

mod dir;
mod warc;
// `pub(crate)` so the unit tests can reach the ZIM run-grouping/key helpers directly.
pub(crate) mod zim;

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, mpsc};

use anyhow::{Result, bail};
use unicode_normalization::{UnicodeNormalization, is_nfc};

pub(crate) use dir::DirSource;
pub(crate) use warc::WarcSource;
pub(crate) use zim::ZimSource;

/// A per-document consumer. Implemented by `convert` (parse to an archive entry) and `stats`
/// (tolerant cardinality counting), and by the small key sinks behind `list`/`extract`.
pub(crate) trait DocSink {
    fn accept(&mut self, key: &str, html: &str);
}

/// Pass-1 tally, reported after a source is opened.
#[derive(Default)]
pub(crate) struct SourceStats {
    /// Documents queued for processing across all runs.
    pub prepared: usize,
    /// Entries skipped as not-an-HTML-document (ZIM metadata/redirects/non-html, …).
    pub ignored: u32,
    /// Documents dropped because their key already appeared (first wins).
    pub collapsed: u32,
}

/// A source of HTML documents grouped into bundle-sized runs. `Sync` so a single source can
/// be driven by many worker threads at once (see [`drive_runs_parallel`]).
pub(crate) trait Source: Sync {
    /// Number of bundle-sized runs.
    fn run_count(&self) -> usize;

    /// Drive run `rank`, calling `sink.accept(key, html)` for each readable document in
    /// order. Returns the count of documents whose bytes could not be read or decoded
    /// (distinct from later parse failures, which the sink itself counts).
    fn drive_run(&self, rank: usize, sink: &mut dyn DocSink) -> u32;

    fn stats(&self) -> &SourceStats;
}

/// Which reader to use for an input path.
#[derive(Clone, Copy)]
pub(crate) enum Format {
    Zim,
    Warc,
    Dir,
}

fn parse_format(s: &str) -> Result<Format> {
    match s.to_ascii_lowercase().as_str() {
        "zim" => Ok(Format::Zim),
        "warc" => Ok(Format::Warc),
        "dir" => Ok(Format::Dir),
        other => bail!("unknown --format '{other}' (expected zim, warc, or dir)"),
    }
}

/// Infer the input format from the path, honouring an explicit `--format` override.
fn detect_format(path: &Path, override_: Option<&str>) -> Result<Format> {
    if let Some(s) = override_ {
        return parse_format(s);
    }
    if path.is_dir() {
        return Ok(Format::Dir);
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.ends_with(".zim") {
        Ok(Format::Zim)
    } else if name.ends_with(".warc") || name.ends_with(".warc.gz") {
        Ok(Format::Warc)
    } else {
        bail!(
            "could not infer the input format of {}; pass --format zim|warc|dir",
            path.display()
        )
    }
}

/// Open `input` as a document source, applying an optional key allow-list and document limit
/// during the source's own pass-1 scan.
pub(crate) fn open_source(
    input: &Path,
    format: Option<&str>,
    wordlist: Option<&HashSet<String>>,
    limit: Option<usize>,
) -> Result<Box<dyn Source>> {
    Ok(match detect_format(input, format)? {
        Format::Zim => Box::new(ZimSource::open(input, wordlist, limit)?),
        Format::Warc => Box::new(WarcSource::open(input, wordlist, limit)?),
        Format::Dir => Box::new(DirSource::open(input, wordlist, limit)?),
    })
}

/// NFC-normalize a string. ZIM titles and the wordlist are compared in NFC.
pub(crate) fn nfc(s: &str) -> String {
    if is_nfc(s) {
        s.to_string()
    } else {
        s.nfc().collect()
    }
}

/// Parse a wordlist file's contents into an NFC-normalized key set (blank lines ignored).
pub(crate) fn parse_wordlist(content: &str) -> HashSet<String> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(nfc)
        .collect()
}

/// Load the optional `--list` allow-list, if a path was given.
pub(crate) fn load_wordlist(path: Option<&Path>) -> Result<Option<HashSet<String>>> {
    match path {
        None => Ok(None),
        Some(p) => {
            let content = std::fs::read_to_string(p)
                .map_err(|e| anyhow::anyhow!("could not read wordlist {}: {e}", p.display()))?;
            Ok(Some(parse_wordlist(&content)))
        }
    }
}

/// A counting semaphore (std has none): caps how many runs are in flight, bounding peak
/// memory to roughly `permits` runs' worth of work.
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

/// Worker count: `available_parallelism`, overridable via `HTMLARC_CONVERT_THREADS` (handy
/// for benchmarking and for asserting output is independent of the thread count).
pub(crate) fn thread_count() -> usize {
    std::env::var("HTMLARC_CONVERT_THREADS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |p| p.get()))
}

/// Run `process(rank)` for every run in parallel, then call `emit` with each result on the
/// coordinator thread in ascending rank order. A permit semaphore caps runs in flight so peak
/// memory is bounded; the deterministic rank order makes the emitted sequence independent of
/// the thread count.
pub(crate) fn drive_runs_parallel<R, P, E>(run_count: usize, process: P, mut emit: E)
where
    R: Send,
    P: Fn(usize) -> R + Sync,
    E: FnMut(R),
{
    if run_count == 0 {
        return;
    }
    let threads = thread_count();
    // >= threads keeps every worker busy; the slack absorbs out-of-order completion so the
    // reorder buffer (and thus peak memory) stays bounded.
    let in_flight_cap = (threads * 2).max(threads + 4);

    let next_rank = AtomicUsize::new(0);
    let permits = Semaphore::new(in_flight_cap);
    let (tx, rx) = mpsc::channel::<(usize, R)>();
    let process = &process;
    let next_rank = &next_rank;
    let permits = &permits;

    std::thread::scope(|scope| {
        for _ in 0..threads {
            let tx = tx.clone();
            scope.spawn(move || {
                loop {
                    // Acquire a permit *before* grabbing work so at most `in_flight_cap` runs
                    // are resident at once.
                    permits.acquire();
                    let rank = next_rank.fetch_add(1, Ordering::SeqCst);
                    if rank >= run_count {
                        permits.release();
                        break;
                    }
                    let result = process(rank);
                    if tx.send((rank, result)).is_err() {
                        permits.release();
                        break;
                    }
                }
            });
        }
        drop(tx);

        // Coordinator: reassemble in ascending rank order; release a permit only as a run is
        // emitted, keeping memory bounded.
        let mut buffer: BTreeMap<usize, R> = BTreeMap::new();
        let mut next_emit = 0usize;
        for (rank, result) in rx {
            buffer.insert(rank, result);
            while let Some(result) = buffer.remove(&next_emit) {
                emit(result);
                next_emit += 1;
                permits.release();
            }
        }
    });
}
