//! Directory source: a tree of saved `.html` / `.htm` files.
//!
//! Pass 1 walks the directory in sorted order and chunks the files into runs of
//! [`BUNDLE_CAP`]; the document key is each file's path relative to the root. Files are read
//! lazily in [`drive_run`](DirSource::drive_run), so only one run's bytes are resident at a
//! time.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use htmlarc_archive::BUNDLE_CAP;
use walkdir::WalkDir;

use super::{DocSink, Source, SourceStats};

/// One document: its key (path relative to the root) and the file to read.
type FileDoc = (String, PathBuf);

pub(crate) struct DirSource {
    runs: Vec<Vec<FileDoc>>,
    stats: SourceStats,
}

fn is_html_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("html") | Some("htm")
    )
}

impl DirSource {
    pub(crate) fn open(
        root: &Path,
        wordlist: Option<&HashSet<String>>,
        limit: Option<usize>,
    ) -> Result<Self> {
        let mut docs: Vec<FileDoc> = Vec::new();
        let mut stats = SourceStats::default();
        let mut seen: HashSet<String> = HashSet::new();

        // Sort entries for a deterministic, reproducible run layout.
        let mut walk: Vec<PathBuf> = WalkDir::new(root)
            .sort_by_file_name()
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file() && is_html_file(e.path()))
            .map(|e| e.into_path())
            .collect();
        walk.sort();

        for path in walk {
            if let Some(lim) = limit
                && docs.len() >= lim
            {
                break;
            }
            let key = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if let Some(set) = wordlist
                && !set.contains(&key)
            {
                stats.ignored += 1;
                continue;
            }
            if !seen.insert(key.clone()) {
                stats.collapsed += 1;
                continue;
            }
            docs.push((key, path));
        }

        stats.prepared = docs.len();
        let runs = docs
            .chunks(BUNDLE_CAP)
            .map(|c| c.to_vec())
            .collect::<Vec<_>>();
        if runs.is_empty() && stats.prepared == 0 {
            return Err(anyhow!(
                "no .html/.htm files found under {}",
                root.display()
            ));
        }
        Ok(Self { runs, stats })
    }
}

impl Source for DirSource {
    fn run_count(&self) -> usize {
        self.runs.len()
    }

    fn drive_run(&self, rank: usize, sink: &mut dyn DocSink) -> u32 {
        let mut read_failed = 0u32;
        for (key, path) in &self.runs[rank] {
            match std::fs::read(path) {
                Ok(bytes) => sink.accept(key, &String::from_utf8_lossy(&bytes), None),
                Err(e) => {
                    eprintln!("could not read {}: {e}", path.display());
                    read_failed += 1;
                }
            }
        }
        read_failed
    }

    fn stats(&self) -> &SourceStats {
        &self.stats
    }
}
