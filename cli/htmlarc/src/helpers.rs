use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::Result;
use htmlarc_archive::Filter;

use crate::source::ArchiveSource;

pub fn create_list_indexes(
    archive: &Arc<ArchiveSource>,
    include: Vec<String>,
    exclude: Vec<String>,
    first_n: Option<usize>,
) -> Result<Vec<usize>> {
    let first_n = first_n.unwrap_or(usize::MAX);

    let filters = Arc::new(Filter::new(include, exclude)?);

    // Fast path: when the include filter restricts by key (a `words:`/`.tsv` rule), only those
    // keys can match — resolve them straight through the keyed index instead of scanning every
    // document. For a pure word/key filter this touches no document blob at all (`keep_key`);
    // otherwise it materializes only the handful of candidate documents to apply the CSS/exclude
    // rules. Reduces a keyed list from an O(n) corpus sweep to O(list) indexed lookups.
    if let Some(keys) = filters.include_keys() {
        // Resolve keys to flat positions first, then evaluate the keep-filter in bundle→doc
        // order: any blob materialization (a mixed `words:` + `css:` filter, via `keep`) then
        // touches each bundle once, where lexical/hash key order would scatter it across bundles
        // (relevant once the reserved per-bundle region holds shared data). A pure word filter
        // short-circuits through `keep_key` and never touches a blob. Positions are pre-sorted,
        // so the kept subset is already in document order for `--first-n`.
        let mut positions: Vec<usize> = keys
            .iter()
            .filter_map(|key| archive.position_for_key(key))
            .collect();
        positions.sort_unstable();
        let mut indexes: Vec<usize> = positions
            .into_iter()
            .filter(|&i| {
                filters
                    .keep_key(archive.key(i))
                    .unwrap_or_else(|| archive.keep(i, &filters))
            })
            .collect();
        indexes.truncate(first_n);
        return Ok(indexes);
    }

    let p_count = thread::available_parallelism().map_or(1, |p| p.get());

    // One whole bundle (up to BUNDLE_CAP docs) is the unit of work — workers steal bundles by
    // index, mirroring the `probe` sweep (`probe::probe`). This keeps each worker on a
    // contiguous bundle→doc range, the natural place to hoist a per-bundle-data load once a
    // later step populates the reserved per-bundle region (instead of re-reading it per doc).
    let bundle_count = archive.bundle_count();
    let mut threads = Vec::new();
    let next_bundle = Arc::new(AtomicUsize::new(0));
    for _ in 0..p_count {
        threads.push(thread::spawn({
            let filters = filters.clone();
            let next_bundle = next_bundle.clone();
            let archive = archive.clone();
            move || {
                let mut indexes = Vec::new();
                loop {
                    let bundle = next_bundle.fetch_add(1, Ordering::Relaxed);

                    if bundle >= bundle_count {
                        break;
                    }

                    for i in archive.bundle_range(bundle) {
                        if archive.keep(i, &filters) {
                            indexes.push(i);
                        }
                    }
                }

                indexes
            }
        }))
    }

    // Collect every kept index, then keep the lowest `first_n` in document order. There is no
    // per-worker early-exit: because workers steal bundles out of order, *which* `first_n`
    // documents won an early-exit race would be non-deterministic (any document could lose),
    // so `--first-n` would return a sorted-but-arbitrary subset — an intermittent
    // `cmd_list_plain` flake. Collect-then-truncate matches the keyed fast path above; the cost
    // is a full `keep()` scan even when `first_n` is small, acceptable for a list/pack command
    // and the price of determinism.
    let mut indexes = threads
        .into_iter()
        .flat_map(|t: JoinHandle<Vec<usize>>| t.join().expect("Failed to process thread"))
        .collect::<Vec<_>>();
    indexes.sort();
    indexes.truncate(first_n);

    Ok(indexes)
}

pub fn create_diff_indexes(
    list_archive: &Arc<ArchiveSource>,
    diff_archive: &ArchiveSource,
) -> Vec<usize> {
    (0..diff_archive.len())
        .filter(|&i| {
            let key = diff_archive.key(i);
            list_archive
                .checksum_for_key(key)
                .is_some_and(|listed| listed != diff_archive.checksum(i))
        })
        .collect()
}
