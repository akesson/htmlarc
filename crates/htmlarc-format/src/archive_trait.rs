use htmlarc_dom::prelude::{DomRead, HtmlElement, HtmlFormat, HtmlTag};

use crate::Filter;

/// One entry — a key plus its DOM — in an [`Archive`].
///
/// Abstracts over the owned ([`crate::HtmlEntry`]) and zero-copy memory-mapped
/// ([`crate::ArchivedHtmlEntry`]) representations: the associated [`Dom`](Self::Dom)
/// is `DomInner` for the former and `ArchivedDomInner` for the latter, and both
/// implement [`DomRead`], so the document is queried and rendered identically.
pub trait ArchiveEntry {
    type Dom: DomRead;

    /// The entry key (e.g. the source file name).
    fn key(&self) -> &str;
    /// Checksum of the stored DOM, for fast archive diffing.
    fn checksum(&self) -> u64;
    /// The entry's document, as something queryable.
    fn dom(&self) -> &Self::Dom;

    /// The document root element.
    fn root(&self) -> HtmlElement<'_, Self::Dom> {
        self.dom().root()
    }

    /// The `<body>` element, if present.
    fn body(&self) -> Option<HtmlElement<'_, Self::Dom>> {
        self.dom()
            .root()
            .forwards()
            .find(|element| element.tag() == HtmlTag::body)
    }

    /// Render the document to an HTML string.
    fn to_html(&self, fmt: HtmlFormat) -> String {
        self.dom().to_html(fmt)
    }
}

/// A queryable archive of HTML documents, addressed by key.
///
/// Implemented by both the in-memory [`crate::HtmlArchive`] and the zero-copy
/// [`crate::MmapArchive`] so callers can be generic over how the bytes are stored
/// — mirroring how [`DomRead`] unifies owned and archived DOMs.
pub trait Archive {
    type Entry: ArchiveEntry;

    /// Number of entries.
    fn len(&self) -> usize;
    /// Look an entry up by key.
    fn get(&self, key: &str) -> Option<&Self::Entry>;
    /// Iterate all entries in key order.
    fn entries(&self) -> impl Iterator<Item = &Self::Entry>;

    /// Whether the archive has no entries.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate all entry keys in order.
    fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries().map(ArchiveEntry::key)
    }

    /// Entries whose key and DOM pass `filter` (its CSS-selector / word predicate).
    fn entries_matching<'a>(
        &'a self,
        filter: &'a Filter,
    ) -> impl Iterator<Item = &'a Self::Entry> {
        self.entries()
            .filter(move |entry| filter.keep(entry.key(), entry.dom()))
    }
}
