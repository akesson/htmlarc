//! The `export` command plus the small ZIM helpers shared by all three commands.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Display};
use std::path::Path;
use std::str::from_utf8;
use std::time::Instant;

use anyhow::{Result, anyhow};
use htmlarc_dom::prelude::HtmlDoc;
use htmlarc_format::ArchiveWriter;
use unicode_normalization::{UnicodeNormalization, is_nfc};
use zim::{MimeType, Namespace, Target, Zim};

use crate::args::Export;

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
    start: Instant,
}

impl Measurements {
    fn new() -> Self {
        Self {
            exported: 0,
            ignored: 0,
            failed: 0,
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

pub(crate) fn run(args: Export) -> Result<()> {
    let Export { file, output, list } = args;
    let zim = open(&file)?;
    let wordlist = match &list {
        Some(path) => Some(load_wordlist(path)?),
        None => None,
    };

    let mut counts = Measurements::new();

    // Pass 1: collect (cluster, blob, title) for html articles that pass the filters.
    // Cheap — only u32s + title strings, no decompression yet.
    let mut groups: BTreeMap<u32, Vec<(u32, String)>> = BTreeMap::new();
    for entry in zim.iterate_by_urls() {
        if !is_content(&entry.namespace) {
            continue; // metadata / fulltext-index / categories: not articles, don't count
        }
        match &entry.target {
            Some(Target::Cluster(c, b)) if html_mime(&entry.mime_type) => {
                let key = key_for(&entry.title, &entry.url);
                if let Some(set) = &wordlist
                    && !set.contains(&key)
                {
                    counts.ignored += 1;
                    continue;
                }
                groups.entry(*c).or_default().push((*b, key));
            }
            // redirects, non-html content (images/css/js), link-targets, deleted entries:
            _ => counts.ignored += 1,
        }
    }

    // Pass 2: read grouped by cluster. Reading every blob of a cluster within one
    // `get_cluster` scope decompresses that cluster once (it memoizes), instead of
    // re-decompressing per article — ~hundreds of articles share a single zstd cluster.
    let mut writer = ArchiveWriter::create(&output)?;
    for (cluster_idx, blobs) in &groups {
        let cluster = match zim.get_cluster(*cluster_idx) {
            Ok(cl) => cl,
            Err(e) => {
                eprintln!("cluster {cluster_idx} read failed: {e:?}");
                counts.failed += blobs.len() as u32;
                continue;
            }
        };
        for (blob, key) in blobs {
            let bytes = match cluster.get_blob(*blob) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("blob read failed for '{key}': {e:?}");
                    counts.failed += 1;
                    continue;
                }
            };
            let html = match from_utf8(bytes.as_ref()) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("'{key}' is not valid utf-8: {e}");
                    counts.failed += 1;
                    continue;
                }
            };
            match HtmlDoc::parse(html) {
                Ok(doc) => {
                    // Stream the parsed doc straight to disk and drop it — only one document
                    // is resident at a time, so peak RSS no longer scales with the corpus.
                    writer.push(key.clone(), doc)?;
                    counts.exported += 1;
                }
                Err(e) => {
                    eprintln!("parse failed for '{key}': {e}");
                    counts.failed += 1;
                }
            }
        }
    }

    // Archive keys are unique, so any same-keyed articles collapsed on push. Surface that.
    let stored = writer.doc_count() as u32;
    let collapsed = writer.collapsed() as u32;
    writer.finish()?;

    println!("{counts}");
    if collapsed > 0 {
        println!(
            "Note: {collapsed} article(s) shared a key and were merged ({stored} unique in archive)."
        );
    }
    Ok(())
}
