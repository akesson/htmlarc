use std::path::Path;

use anyhow::{Context, Result};
use htmlarc_archive::{Filter, HtmlArchive, MmapArchive};
use htmlarc_dom::prelude::HtmlFormat;

/// The archive a CLI command operates on.
///
/// A packed `.htmlarc` is opened **zero-copy via [`MmapArchive`]** (instant open, low
/// resident memory); a directory or single `.html` file is parsed into an owned
/// [`HtmlArchive`]. Commands query through this enum so they don't care which.
pub enum ArchiveSource {
    Owned(HtmlArchive),
    Mapped(MmapArchive),
}

impl ArchiveSource {
    /// Open a source: parse directories / `.html` files into owned memory; memory-map
    /// a packed `.htmlarc`.
    pub fn open(source: &Path) -> Result<Self> {
        if is_parsed_source(source) {
            let archive = HtmlArchive::open(source)
                .with_context(|| format!("opening source {}", source.display()))?;
            Ok(Self::Owned(archive))
        } else {
            let mmap = MmapArchive::open(source)
                .with_context(|| format!("memory-mapping source {}", source.display()))?;
            Ok(Self::Mapped(mmap))
        }
    }

    /// Wrap an already-built owned archive (used by in-memory test fixtures).
    pub fn from_owned(archive: HtmlArchive) -> Self {
        Self::Owned(archive)
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Owned(a) => a.len(),
            Self::Mapped(m) => m.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Number of bundles. Lets the parallel `list` sweep steal whole bundles (matching the
    /// `probe` sweep) instead of individual document indices.
    pub fn bundle_count(&self) -> usize {
        match self {
            Self::Owned(a) => a.bundle_count(),
            Self::Mapped(m) => m.bundle_count(),
        }
    }

    /// The half-open flat (bundle→doc) position range covered by bundle `b`.
    pub fn bundle_range(&self, b: usize) -> std::ops::Range<usize> {
        match self {
            Self::Owned(a) => a.bundle_range(b),
            Self::Mapped(m) => m.bundle_range(b),
        }
    }

    pub fn key(&self, i: usize) -> &str {
        match self {
            Self::Owned(a) => a[i].key.as_str(),
            // Served straight from the footer directory — no document blob is touched.
            Self::Mapped(m) => m.key_at(i),
        }
    }

    pub fn checksum(&self, i: usize) -> u64 {
        match self {
            Self::Owned(a) => a[i].checksum,
            // Served straight from the footer directory — no document blob is touched.
            Self::Mapped(m) => m.checksum_at(i),
        }
    }

    pub fn to_html(&self, i: usize, fmt: HtmlFormat) -> String {
        match self {
            Self::Owned(a) => a[i].html.to_html(fmt),
            Self::Mapped(m) => m[i].to_html(fmt),
        }
    }

    /// Whether the document at `i` passes the filter (CSS/word predicate), evaluated
    /// directly against the owned or memory-mapped DOM.
    pub fn keep(&self, i: usize, filter: &Filter) -> bool {
        match self {
            Self::Owned(a) => filter.keep(&a[i].key, &a[i].html),
            Self::Mapped(m) => {
                let e = &m[i];
                filter.keep(e.key(), &e.html)
            }
        }
    }

    pub fn checksum_for_key(&self, key: &str) -> Option<u64> {
        match self {
            Self::Owned(a) => a.get(key).map(|e| e.checksum),
            // Footer-only lookup: the checksum lives in the directory, not the blob.
            Self::Mapped(m) => m.checksum_for_key(key),
        }
    }

    /// The flat position of `key` via the keyed index, or `None` if absent — no blob touched.
    /// A keyed word-list search resolves through this instead of scanning every position.
    pub fn position_for_key(&self, key: &str) -> Option<usize> {
        match self {
            Self::Owned(a) => a.position_for_key(key),
            Self::Mapped(m) => m.position_for_key(key),
        }
    }

    /// Render the document with the given `key`, if present. `Err` means the matching blob
    /// failed validation (memory-mapped reads validate a document only when fetched).
    pub fn html_for_key(&self, key: &str, fmt: HtmlFormat) -> Result<Option<String>> {
        match self {
            Self::Owned(a) => Ok(a.get(key).map(|e| e.html.to_html(fmt))),
            Self::Mapped(m) => Ok(m.get(key)?.map(|e| e.to_html(fmt))),
        }
    }
}

/// Whether a source should be parsed into an owned archive (a directory or a single
/// HTML file) rather than memory-mapped as a packed `.htmlarc`.
pub(crate) fn is_parsed_source(path: &Path) -> bool {
    path.is_dir() || is_html_path(path)
}

fn is_html_path(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}
