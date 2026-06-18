use crate::error::ArchiveErr;
use htmlarc_dom::prelude::*;
use rkyv::rancor::Error;
use rkyv::util::AlignedVec;
use rkyv::{Archive, Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use unicode_segmentation::UnicodeSegmentation;

/// One document already serialized to its on-disk rkyv form, plus the doc-table metadata.
///
/// Produced off the coordinator by [`HtmlEntry::into_serialized`] so the streaming
/// [`ArchiveWriter`](crate::ArchiveWriter) only appends bytes. This keeps the heavy
/// `DomInner` off the single coordinator thread and out of the in-flight/reorder set — the
/// bytes are the compact stored form (~3× smaller than the live DOM).
pub struct SerializedEntry {
    /// The entry key.
    pub key: String,
    /// Grapheme count of `key`.
    pub key_len: u16,
    /// Checksum of the stored DOM.
    pub checksum: u64,
    /// The rkyv-archived [`HtmlEntry`] blob, ready to write verbatim. Its text/comment pool has
    /// been moved out into [`strings`](Self::strings), so the blob is string-less; the node text
    /// ranges stay valid against the relocated pool.
    pub bytes: AlignedVec,
    /// The document's text/comment pool, relocated out of `bytes` so the writer can pack a whole
    /// bundle's pools into the bundle's data region.
    pub strings: Vec<u8>,
}

/// One pre-parsed HTML document in an archive, addressed by `key`.
///
/// Entries are kept sorted by (`key_len`, `key`) so [`crate::HtmlArchive::get`] can
/// binary-search them.
#[derive(Archive, Deserialize, Serialize)]
pub struct HtmlEntry {
    /// The entry key (e.g. the source file name).
    pub key: String,
    /// Grapheme count of `key`, used as the primary sort/search dimension.
    pub key_len: u16,
    /// Checksum of the stored DOM, used for fast archive diffing.
    pub checksum: u64,
    pub html: DomInner,
}

impl HtmlEntry {
    pub fn new(key: String, html: HtmlDoc) -> Self {
        // store the most compact node width; the checksum then covers the stored form
        let dom = html.dom().into_optimal_width();
        let mut hasher = seahash::SeaHasher::new();
        dom.hash(&mut hasher);
        let checksum = hasher.finish();
        let key_len = key.graphemes(true).count() as u16;
        Self {
            key,
            key_len,
            checksum,
            html: dom,
        }
    }

    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    pub fn checksum(&self) -> u64 {
        self.checksum
    }

    pub fn root(&self) -> HtmlElement<'_, DomInner> {
        self.html.root()
    }

    pub fn body(&self) -> Option<HtmlElement<'_, DomInner>> {
        self.html
            .root()
            .forwards()
            .find(|element| element.tag() == HtmlTag::body)
    }

    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        self.html.to_html(fmt)
    }

    /// Serialize to the on-disk rkyv form, returning the bytes plus the doc-table metadata.
    /// Call this on a worker thread so the coordinator never touches the live `DomInner`; the
    /// returned [`SerializedEntry`] is what [`ArchiveWriter::push_serialized`](crate::ArchiveWriter::push_serialized)
    /// appends. Consumes `self` so the heavy DOM is dropped the moment its bytes exist.
    pub fn into_serialized(mut self) -> Result<SerializedEntry, ArchiveErr> {
        // Relocate the text pool out of the blob first: the bytes go to the bundle's data region
        // and the per-document blob is serialized string-less (its node ranges still resolve
        // against the relocated segment).
        let strings = self.html.take_string_pool();
        let bytes =
            rkyv::to_bytes::<Error>(&self).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        Ok(SerializedEntry {
            key: self.key,
            key_len: self.key_len,
            checksum: self.checksum,
            bytes,
            strings,
        })
    }
}

/// Zero-copy accessors on the rkyv-archived entry. Key and checksum read straight from the blob;
/// the document itself is reached through [`bind`](Self::bind), because a relocated document's
/// text lives in its bundle (not its own blob) and must be supplied as a [`StringSource`].
impl ArchivedHtmlEntry {
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    pub fn checksum(&self) -> u64 {
        self.checksum.to_native()
    }

    /// Bind this entry's topology to its text source (its bundle's per-document segment),
    /// yielding a readable, queryable [`ArchivedDom`].
    pub fn bind<'a>(&'a self, strings: StringSource<'a>) -> ArchivedDom<'a> {
        self.html.with_strings(strings)
    }
}

impl Eq for HtmlEntry {}

impl PartialEq for HtmlEntry {
    fn eq(&self, other: &Self) -> bool {
        self.key_len == other.key_len && self.key == other.key && self.checksum == other.checksum
    }
}

impl Ord for HtmlEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.key_len
            .cmp(&other.key_len)
            .then_with(|| self.key.cmp(&other.key))
    }
}

impl PartialOrd for HtmlEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl crate::ArchiveEntry for HtmlEntry {
    fn key(&self) -> &str {
        self.key.as_str()
    }

    fn checksum(&self) -> u64 {
        self.checksum
    }
}

impl crate::ArchiveEntry for ArchivedHtmlEntry {
    fn key(&self) -> &str {
        self.key.as_str()
    }

    fn checksum(&self) -> u64 {
        self.checksum.to_native()
    }
}
