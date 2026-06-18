use crate::{ArchiveErr, Filter};

/// One entry — a key plus its DOM — in an [`Archive`].
///
/// Abstracts over the owned ([`crate::HtmlEntry`]) and zero-copy memory-mapped
/// ([`crate::ArchivedHtmlEntry`]) representations. It exposes only the metadata that lives
/// alongside every document (key, checksum); the document body is reached differently per
/// backing, because a memory-mapped entry's text is relocated into its bundle and must be bound
/// to a text source first (owned: [`crate::HtmlEntry::root`]; mmap: [`crate::MmapArchive::doc`]).
pub trait ArchiveEntry {
    /// The entry key (e.g. the source file name).
    fn key(&self) -> &str;
    /// Checksum of the stored DOM, for fast archive diffing.
    fn checksum(&self) -> u64;
}

/// A queryable archive of HTML documents, addressed by key.
///
/// Implemented by both the in-memory [`crate::HtmlArchive`] and the zero-copy
/// [`crate::MmapArchive`] so callers can be generic over how the bytes are stored.
pub trait Archive {
    type Entry: ArchiveEntry;

    /// Number of entries.
    fn len(&self) -> usize;
    /// Look an entry up by key. `Ok(None)` means absent; `Err` means the matching document
    /// blob failed validation (distinguishing corruption from a missing key — the lazy
    /// memory-mapped reader only validates a blob when it is actually fetched).
    fn get(&self, key: &str) -> Result<Option<&Self::Entry>, ArchiveErr>;
    /// Iterate all entries in key order. (Bulk iteration over a corrupt blob panics — every
    /// index is valid by construction, so corruption here is an exceptional, abort-worthy
    /// condition rather than a normal `None`.)
    fn entries(&self) -> impl Iterator<Item = &Self::Entry>;

    /// Whether the archive has no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate all entry keys in order.
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries().map(ArchiveEntry::key)
    }

    /// The keys of every document that passes `filter`. Required (not a default) because a
    /// memory-mapped archive must bind each document to its bundle's relocated text before the
    /// filter's CSS rules can inspect it — which only that backing knows how to do. A key-only
    /// filter ([`Filter::keep_key`]) short-circuits without touching the document body.
    fn entries_matching<'a>(&'a self, filter: &'a Filter) -> Vec<&'a str>;
}
