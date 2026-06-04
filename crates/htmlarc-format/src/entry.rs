use htmlarc_dom::prelude::*;
use rkyv::{Archive, Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use unicode_segmentation::UnicodeSegmentation;

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
        let dom = html.inner();
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

    pub fn root(&self) -> HtmlElement<'_, DomInner> {
        self.html.root()
    }

    pub fn body(&self) -> Option<HtmlElement<'_, DomInner>> {
        self.html
            .root()
            .forwards()
            .find(|element| element.tag() == HtmlTag::body)
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
