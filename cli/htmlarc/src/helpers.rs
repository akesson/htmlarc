use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
};

use anyhow::{Context, Result};
use htmlarc_format::HtmlArchive;

use htmlarc_format::Filter;

pub fn read_arch(path: &str) -> Result<HtmlArchive> {
    HtmlArchive::read_from(path).context(format!("Failed to read archive: {path}"))
}

pub fn create_list_indexes(
    archive: &Arc<HtmlArchive>,
    include: Vec<String>,
    exclude: Vec<String>,
    first_n: Option<usize>,
) -> Result<Vec<usize>> {
    let first_n = first_n.unwrap_or(usize::MAX);

    let filters = Arc::new(Filter::new(include, exclude)?);

    let p_count = thread::available_parallelism().map_or(1, |p| p.get());

    let mut threads = Vec::new();
    let index = Arc::new(AtomicUsize::new(0));
    let count = Arc::new(AtomicUsize::new(0));
    for _ in 0..p_count {
        threads.push(thread::spawn({
            let filters = filters.clone();
            let index = index.clone();
            let count = count.clone();
            let archive = archive.clone();
            move || {
                let mut indexes = Vec::new();
                loop {
                    let i: usize = index.fetch_add(1, Ordering::Relaxed);

                    if i >= archive.len() {
                        break;
                    }

                    let entry = &archive[i];

                    if filters.keep(&entry.key, &entry.html) {
                        let count: usize = count.fetch_add(1, Ordering::Relaxed);
                        if count >= first_n {
                            break;
                        }
                        indexes.push(i);
                    }
                }

                indexes
            }
        }))
    }

    let mut indexes = threads
        .into_iter()
        .flat_map(|t: JoinHandle<Vec<usize>>| t.join().expect("Failed to process thread"))
        .collect::<Vec<_>>();
    indexes.sort();

    Ok(indexes)
}

pub fn create_diff_indexes(
    list_archive: &Arc<HtmlArchive>,
    diff_archive: &HtmlArchive,
) -> Vec<usize> {
    diff_archive
        .entries()
        .enumerate()
        .filter_map(|(i, entry)| {
            if let Some(e) = list_archive.get(&entry.key) {
                entry.checksum != e.checksum
            } else {
                false
            }
            .then_some(i)
        })
        .collect::<Vec<_>>()
}
