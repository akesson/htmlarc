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
    /// The rkyv-archived [`HtmlEntry`] blob, ready to write verbatim.
    pub bytes: AlignedVec,
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
    pub fn into_serialized(self) -> Result<SerializedEntry, ArchiveErr> {
        let bytes =
            rkyv::to_bytes::<Error>(&self).map_err(|e| ArchiveErr::Serialize(e.to_string()))?;
        Ok(SerializedEntry {
            key: self.key,
            key_len: self.key_len,
            checksum: self.checksum,
            bytes,
        })
    }
}

/// Zero-copy accessors on the rkyv-archived entry, so a memory-mapped archive reads
/// keys, checksums, and renders documents without deserializing anything.
impl ArchivedHtmlEntry {
    pub fn key(&self) -> &str {
        self.key.as_str()
    }

    pub fn checksum(&self) -> u64 {
        self.checksum.to_native()
    }

    pub fn root(&self) -> HtmlElement<'_, ArchivedDomInner> {
        self.html.root()
    }

    pub fn body(&self) -> Option<HtmlElement<'_, ArchivedDomInner>> {
        self.html
            .root()
            .forwards()
            .find(|element| element.tag() == HtmlTag::body)
    }

    pub fn to_html(&self, fmt: HtmlFormat) -> String {
        self.html.to_html(fmt)
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
    type Dom = DomInner;

    fn key(&self) -> &str {
        self.key.as_str()
    }

    fn checksum(&self) -> u64 {
        self.checksum
    }

    fn dom(&self) -> &DomInner {
        &self.html
    }
}

impl crate::ArchiveEntry for ArchivedHtmlEntry {
    type Dom = ArchivedDomInner;

    fn key(&self) -> &str {
        self.key.as_str()
    }

    fn checksum(&self) -> u64 {
        self.checksum.to_native()
    }

    fn dom(&self) -> &ArchivedDomInner {
        &self.html
    }
}
