use std::path::Path;

use anyhow::{Context, Result};
use htmlarc_dom::prelude::HtmlFormat;
use htmlarc_format::{Filter, HtmlArchive, MmapArchive};

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

    pub fn key(&self, i: usize) -> &str {
        match self {
            Self::Owned(a) => a[i].key.as_str(),
            Self::Mapped(m) => m[i].key(),
        }
    }

    pub fn checksum(&self, i: usize) -> u64 {
        match self {
            Self::Owned(a) => a[i].checksum,
            Self::Mapped(m) => m[i].checksum(),
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
            Self::Mapped(m) => filter.keep(m[i].key(), &m[i].html),
        }
    }

    pub fn checksum_for_key(&self, key: &str) -> Option<u64> {
        match self {
            Self::Owned(a) => a.get(key).map(|e| e.checksum),
            Self::Mapped(m) => m.get(key).map(|e| e.checksum()),
        }
    }

    /// Render the document with the given `key`, if present.
    pub fn html_for_key(&self, key: &str, fmt: HtmlFormat) -> Option<String> {
        match self {
            Self::Owned(a) => a.get(key).map(|e| e.html.to_html(fmt)),
            Self::Mapped(m) => m.get(key).map(|e| e.to_html(fmt)),
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
