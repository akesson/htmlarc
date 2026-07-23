//! ZIM source: the Kiwix/Wikipedia offline format.
//!
//! Pass 1 (in [`ZimSource::open`]) scans the directory and builds an ordered, de-duplicated
//! per-cluster work list — only `u32`s and key strings, no decompression — then groups
//! consecutive clusters into cluster-aligned runs of about [`BUNDLE_CAP`] documents. Each run
//! is one archive bundle. [`drive_run`](ZimSource::drive_run) decompresses a run's clusters
//! (each exactly once) and streams their HTML through the sink, so the heavy parse work stays
//! on the caller's worker thread.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::str::from_utf8;

use anyhow::{Result, anyhow};
use htmlarc_archive::BUNDLE_CAP;
use zim::{MimeType, Namespace, Target, Zim};

use super::{DocSink, Source, SourceStats, nfc};

/// One cluster of work: its ZIM cluster index plus the `(blob index, key)` of every HTML
/// article it holds, in directory order.
pub(crate) type ClusterWork = (u32, Vec<(u32, String)>);

/// A run of consecutive clusters forming one archive bundle (~[`BUNDLE_CAP`] docs).
pub(crate) type Run = Vec<ClusterWork>;

/// Articles live in the `A` namespace (old scheme) or `C`/UserContent (new scheme).
pub(crate) fn is_content(ns: &Namespace) -> bool {
    matches!(ns, Namespace::Articles | Namespace::UserContent)
}

/// True only for `text/html` entries (skips images, css, js, redirects, …).
pub(crate) fn html_mime(m: &MimeType) -> bool {
    matches!(m, MimeType::Type(s) if s.starts_with("text/html"))
}

/// The archive key for an entry: its NFC title, falling back to the URL slug when the title
/// is empty (many ZIM HTML resources have no title but a unique URL).
pub(crate) fn key_for(title: &str, url: &str) -> String {
    nfc(if title.is_empty() { url } else { title })
}

/// Group the ascending, de-duplicated cluster work list into runs of about [`BUNDLE_CAP`]
/// documents. Clusters are never split, so a run is sealed once it reaches the cap — making
/// it `BUNDLE_CAP` rounded up to a cluster boundary — and the final run takes the remainder.
/// The grouping depends only on the work list, so the bundles are deterministic.
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

pub(crate) struct ZimSource {
    zim: Zim,
    runs: Vec<Run>,
    stats: SourceStats,
}

impl ZimSource {
    /// Open a ZIM and build its cluster-aligned runs (pass 1).
    pub(crate) fn open(
        path: &Path,
        wordlist: Option<&HashSet<String>>,
        limit: Option<usize>,
    ) -> Result<Self> {
        let zim =
            Zim::new(path).map_err(|e| anyhow!("could not open ZIM {}: {e:?}", path.display()))?;

        let mut stats = SourceStats::default();
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
                        stats.ignored += 1;
                        continue;
                    }
                    // Dedup here (first wins, in directory order) so the bundle layout is
                    // fully deterministic.
                    if !seen.insert(key.clone()) {
                        stats.collapsed += 1;
                        continue;
                    }
                    groups.entry(*c).or_default().push((*b, key));
                    accepted += 1;
                }
                _ => stats.ignored += 1,
            }
        }

        stats.prepared = accepted;
        let runs = group_into_runs(groups.into_iter().collect());
        Ok(Self { zim, runs, stats })
    }
}

impl Source for ZimSource {
    fn run_count(&self) -> usize {
        self.runs.len()
    }

    fn drive_run(&self, rank: usize, sink: &mut dyn DocSink) -> u32 {
        let mut read_failed = 0u32;
        for (cluster_idx, blobs) in &self.runs[rank] {
            let cluster = match self.zim.get_cluster(*cluster_idx) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("cluster {cluster_idx} read failed: {e:?}");
                    read_failed += blobs.len() as u32;
                    continue;
                }
            };
            for (blob, key) in blobs {
                let bytes = match cluster.get_blob(*blob) {
                    Ok(b) => b,
                    Err(e) => {
                        eprintln!("blob read failed for '{key}': {e:?}");
                        read_failed += 1;
                        continue;
                    }
                };
                match from_utf8(bytes.as_ref()) {
                    Ok(html) => sink.accept(key, html, None),
                    Err(e) => {
                        eprintln!("'{key}' is not valid utf-8: {e}");
                        read_failed += 1;
                    }
                }
            }
        }
        read_failed
    }

    fn stats(&self) -> &SourceStats {
        &self.stats
    }
}
