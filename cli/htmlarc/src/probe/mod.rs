mod expression;
mod format;
mod node_counter;
#[cfg(test)]
mod tests;

use std::{
    collections::HashSet,
    mem,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

pub use expression::ProbeExpression;
pub(crate) use format::*;
use node_counter::CountedNodes;

use crate::MmapArchive;
use crate::args::Probe;
use anyhow::{Context, Result, anyhow};

/// Run the `probe` subcommand: open the source, parse probe expressions, count matches
/// across all documents, and print the aggregated tree.
pub fn run(args: Probe) -> Result<()> {
    let Probe {
        source,
        include,
        exclude,
        probe: exprs,
    } = args;

    // Leak the expression strings first so the parsed selectors can borrow them for 'static.
    let exprs: &'static Vec<String> = Box::leak(Box::new(exprs));
    let expressions = exprs
        .iter()
        .map(|e| ProbeExpression::try_from(e.as_str()).map_err(|e| anyhow!(e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    let expressions: &'static [ProbeExpression<'static>] = Box::leak(Box::new(expressions));

    let filters: &'static Filter = Box::leak(Box::new(Filter::new(include, exclude)?));

    // A packed `.htmlarc` is probed zero-copy via mmap; a directory / `.html` file is
    // parsed into an owned archive. Both leak for `'static` so the worker threads can
    // share them.
    let counted = if crate::source::is_parsed_source(&source) {
        let archive: &'static HtmlArchive =
            Box::leak(Box::new(HtmlArchive::open(&source).with_context(|| {
                format!("opening source {}", source.display())
            })?));
        probe(archive, expressions, filters)
    } else {
        let archive: &'static MmapArchive =
            Box::leak(Box::new(MmapArchive::open(&source).with_context(|| {
                format!("memory-mapping source {}", source.display())
            })?));
        probe(archive, expressions, filters)
    };

    println!("{}", counted.to_pretty_string());
    Ok(())
}

pub fn probe<A: ProbeArchive>(
    archive: &'static A,
    expressions: &'static [ProbeExpression<'static>],
    filters: &'static Filter,
) -> CountedNodes {
    // Fast path: an include filter that restricts by key can only match those keys — resolve them
    // through the keyed index and skip the bundle sweep (and every other document's blob) entirely.
    if let Some(keys) = filters.include_keys() {
        return archive.probe_keys(keys, expressions, filters);
    }

    let bundle_count = archive.bundle_count();
    let thread_count = thread::available_parallelism().map_or(1, |p| p.get());

    // One whole bundle (up to BUNDLE_CAP docs) is the unit of work — workers steal bundles by
    // index. This iterates strictly bundle→doc and gives each thread a self-contained bundle,
    // the natural place to attach per-bundle data in a later step.
    let next_bundle = Arc::new(AtomicUsize::new(0));

    let mut threads = Vec::with_capacity(thread_count);

    for _thread in 0..thread_count {
        let next_bundle = next_bundle.clone();

        threads.push(thread::spawn(move || {
            let mut counters = Vec::new();
            loop {
                let bundle = next_bundle.fetch_add(1, Ordering::SeqCst);
                if bundle >= bundle_count {
                    break;
                }
                let counted = archive.probe_bundle(bundle, expressions, filters);
                counters.push((bundle, counted));
            }
            counters
        }));
    }

    let mut counters = threads
        .into_iter()
        .flat_map(|t: JoinHandle<Vec<(usize, CountedNodes)>>| {
            t.join().expect("Failed to process probe threads")
        })
        .collect::<Vec<_>>();

    // Merge in ascending bundle order so the aggregated tree is deterministic regardless of how
    // bundles were distributed across threads.
    counters.sort_by_key(|(a, _)| *a);

    let mut counter = counters
        .first_mut()
        .map_or(CountedNodes::default(), |(_, c)| mem::take(c));

    for (_, c) in counters.into_iter().skip(1) {
        counter += c;
    }

    counter
}

/// A source the `probe` sweep can run over in parallel — owned or memory-mapped.
/// Each worker analyzes one whole bundle, so this only needs `bundle_count` + per-bundle
/// analysis, dispatched to the concrete (owned vs archived) entry type.
pub trait ProbeArchive: Sync {
    fn bundle_count(&self) -> usize;
    fn probe_bundle<'a>(
        &'a self,
        bundle: usize,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes;

    /// Probe only the documents whose key is in `keys`, resolved through the keyed index — the
    /// fast path when an include filter restricts by key. Skips bundle iteration entirely, so it
    /// touches only the candidate documents' blobs instead of the whole corpus. Keys are processed
    /// in sorted order so the aggregated result is deterministic.
    fn probe_keys<'a>(
        &'a self,
        keys: &HashSet<String>,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes;
}

impl ProbeArchive for HtmlArchive {
    fn bundle_count(&self) -> usize {
        self.bundles().len()
    }

    fn probe_bundle<'a>(
        &'a self,
        bundle: usize,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes {
        let mut counter = CountedNodes::default();
        for doc in self.bundles()[bundle].entries() {
            if filters.keep(&doc.key, &doc.html) {
                counter.analyze_html(&doc.key, &doc.root(), expressions);
            } else {
                debug!("Skipping: {}", doc.key);
            }
        }
        counter
    }

    fn probe_keys<'a>(
        &'a self,
        keys: &HashSet<String>,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes {
        // Resolve keys to flat positions and process in bundle→doc order (matching the bulk
        // `probe_bundle` sweep), so a future per-bundle-data load touches each bundle once
        // instead of re-reading it for keys scattered by lexical order. `position_for_key` is
        // footer-only, so absent keys drop out here without touching a blob.
        let mut positions: Vec<usize> = keys
            .iter()
            .filter_map(|key| self.position_for_key(key))
            .collect();
        positions.sort_unstable();
        let mut counter = CountedNodes::default();
        for i in positions {
            let doc = &self[i];
            if filters.keep(&doc.key, &doc.html) {
                counter.analyze_html(&doc.key, &doc.root(), expressions);
            }
        }
        counter
    }
}

impl ProbeArchive for MmapArchive {
    fn bundle_count(&self) -> usize {
        MmapArchive::bundle_count(self)
    }

    fn probe_bundle<'a>(
        &'a self,
        bundle: usize,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes {
        let mut counter = CountedNodes::default();
        for i in self.bundle_range(bundle) {
            let doc = &self[i];
            let key = doc.key();
            if filters.keep(key, &doc.html) {
                counter.analyze_html(key, &doc.root(), expressions);
            } else {
                debug!("Skipping: {key}");
            }
        }
        counter
    }

    fn probe_keys<'a>(
        &'a self,
        keys: &HashSet<String>,
        expressions: &[ProbeExpression<'a>],
        filters: &Filter,
    ) -> CountedNodes {
        // Resolve keys to flat positions and process in bundle→doc order (matching the bulk
        // `probe_bundle` sweep), so a future per-bundle-data load touches each bundle once
        // instead of re-reading it for keys scattered by lexical order. `position_for_key` is
        // footer-only, so an absent key drops out here without a blob fetch; a present key that
        // points at a corrupt blob still aborts via the positional index (the same abort-worthy
        // stance as the bulk sweep — every blob a present key points at is valid by construction).
        let mut positions: Vec<usize> = keys
            .iter()
            .filter_map(|key| self.position_for_key(key))
            .collect();
        positions.sort_unstable();
        let mut counter = CountedNodes::default();
        for i in positions {
            let doc = &self[i];
            let k = doc.key();
            if filters.keep(k, &doc.html) {
                counter.analyze_html(k, &doc.root(), expressions);
            }
        }
        counter
    }
}
