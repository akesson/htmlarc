mod expression;
mod format;
mod node_counter;
#[cfg(test)]
mod tests;

use std::{
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

    let archive: &'static HtmlArchive =
        Box::leak(Box::new(HtmlArchive::open(&source).with_context(|| {
            format!("opening source {}", source.display())
        })?));

    // Leak the expression strings first so the parsed selectors can borrow them for 'static.
    let exprs: &'static Vec<String> = Box::leak(Box::new(exprs));
    let expressions = exprs
        .iter()
        .map(|e| ProbeExpression::try_from(e.as_str()).map_err(|e| anyhow!(e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    let expressions: &'static [ProbeExpression<'static>] = Box::leak(Box::new(expressions));

    let filters: &'static Filter = Box::leak(Box::new(Filter::new(include, exclude)?));

    let counted = probe(archive, expressions, filters);
    println!("{}", counted.to_pretty_string());
    Ok(())
}

pub fn probe(
    archive: &'static HtmlArchive,
    expressions: &'static [ProbeExpression<'static>],
    filters: &'static Filter,
) -> CountedNodes<'static> {
    let entry_count = archive.len();
    let thread_count = thread::available_parallelism().map_or(1, |p| p.get());

    // 250 entries at roughly 0,2ms/entry gives 50ms per chunk
    const CHUNK_SIZE: usize = 250;
    let chunk_index = Arc::new(AtomicUsize::new(0));
    // when the entry count is divisible by the chunk size, we'll get an extra chunk,
    // but that's handled later.
    let chunk_max_index = entry_count / CHUNK_SIZE + 1;

    let mut threads = Vec::with_capacity(thread_count);

    for _thread in 0..thread_count {
        let chunk_index = chunk_index.clone();

        threads.push(thread::spawn(move || {
            let mut counters = Vec::new();
            loop {
                let my_chunk_index = chunk_index.fetch_add(1, Ordering::SeqCst);
                if my_chunk_index >= chunk_max_index {
                    break;
                }
                let start = my_chunk_index * CHUNK_SIZE;
                let end = (start + CHUNK_SIZE).min(entry_count);
                let counted =
                    probe_word_slice(archive.entries[start..end].iter(), expressions, filters);
                counters.push((my_chunk_index, counted));
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

    counters.sort_by_key(|(a, _)| *a);

    let mut counter = counters
        .first_mut()
        .map_or(CountedNodes::default(), |(_, c)| mem::take(c));

    for (_, c) in counters.into_iter().skip(1) {
        counter += c;
    }

    counter
}

pub fn probe_word_slice<'a>(
    iter: impl Iterator<Item = &'a HtmlEntry>,
    expressions: &[ProbeExpression<'a>],
    filters: &'a Filter,
) -> CountedNodes<'a> {
    let mut counter = CountedNodes::default();
    for doc in iter {
        if filters.keep(&doc.key, &doc.html) {
            counter.analyze_html(&doc.key, &doc.root(), expressions);
        } else {
            debug!("Skipping: {}", doc.key);
        }
    }
    counter
}
