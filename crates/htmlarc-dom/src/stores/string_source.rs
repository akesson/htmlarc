use std::cell::OnceCell;
use std::ops::Range;

/// Borrowed, read-only source of a document's text/comment pool, the single seam through
/// which [`DomView`](crate::dom::DomView) reads text. It abstracts *how* the bytes are stored
/// so the query layer is agnostic to it:
///
/// - [`Plain`](Self::Plain): the bytes are available verbatim — an owned `Vec` (a live
///   document) or an uncompressed memory-mapped slice (a relocated per-bundle pool). Reads are
///   zero-copy. This is the only arm used in production today.
/// - [`Lazy`](Self::Lazy): the bytes live in a compressed `frame`; the first read inflates it
///   once into `buf` and every read slices that buffer at `base + range`. A bundle is always
///   processed by a single thread, so a non-synchronized [`OnceCell`] is sufficient. `decode`
///   is injected by the archive layer so this crate stays codec-agnostic (a `fn` pointer keeps
///   the enum `Copy`). Defined and tested, but dormant until per-bundle compression lands.
///
/// All byte offsets handed to [`get`](Self::get) are document-local; for `Plain` the slice is
/// already narrowed to the document, and for `Lazy` `base` locates the document within the
/// shared inflated frame.
/// The inflate state behind [`StringSource::Lazy`], held behind a shared reference so the enum
/// stays small (two words) — keeping the hot `Plain` path, and the `Copy` [`DomView`] that
/// carries it, as cheap as a bare slice. `buf` is the once-per-bundle decompression cache;
/// `base`/`len` locate this document within the inflated frame.
pub struct LazyState<'a> {
    pub buf: &'a OnceCell<Vec<u8>>,
    pub frame: &'a [u8],
    pub base: u32,
    pub len: u32,
    pub decode: fn(&[u8]) -> Vec<u8>,
}

#[derive(Clone, Copy)]
pub enum StringSource<'a> {
    Plain(&'a [u8]),
    Lazy(&'a LazyState<'a>),
}

impl<'a> StringSource<'a> {
    /// A source over verbatim bytes (owned `Vec` or uncompressed mmap slice).
    pub fn plain(bytes: &'a [u8]) -> Self {
        StringSource::Plain(bytes)
    }

    /// A lazily-inflated source over a compressed frame (the per-document view of a shared
    /// [`LazyState`], whose `base`/`len` locate this document within the inflated buffer).
    pub fn lazy(state: &'a LazyState<'a>) -> Self {
        StringSource::Lazy(state)
    }

    /// The text for `range` (document-local byte offsets). UTF-8 is an invariant of the stored
    /// pool, so the slice is decoded unchecked — identical to the previous flat-pool accessor.
    pub(crate) fn get(&self, range: Range<u32>) -> &'a str {
        match *self {
            StringSource::Plain(bytes) => unchecked_str(bytes, range.start, range.end),
            StringSource::Lazy(s) => {
                debug_assert!(range.end <= s.len, "range past this document's segment");
                let inflated = s.buf.get_or_init(|| (s.decode)(s.frame));
                unchecked_str(inflated, s.base + range.start, s.base + range.end)
            }
        }
    }

    /// Copy out this document's entire text segment, inflating if needed. Used when an archived
    /// document is materialised into an owned, editable [`DomInner`](crate::dom::DomInner) (the
    /// archived→owned `repackage` path), which needs a fresh owned pool.
    pub fn materialize(&self) -> Vec<u8> {
        match *self {
            StringSource::Plain(bytes) => bytes.to_vec(),
            StringSource::Lazy(s) => {
                let inflated = s.buf.get_or_init(|| (s.decode)(s.frame));
                inflated[s.base as usize..(s.base + s.len) as usize].to_vec()
            }
        }
    }

    /// The whole `Plain` segment as bytes (test-only; backs the owned-vs-archived byte-identity
    /// proof in `dom_inner`).
    #[cfg(test)]
    pub(crate) fn as_bytes(&self) -> &'a [u8] {
        match *self {
            StringSource::Plain(bytes) => bytes,
            StringSource::Lazy(_) => panic!("as_bytes is only meaningful for a Plain source"),
        }
    }
}

fn unchecked_str(bytes: &[u8], start: u32, end: u32) -> &str {
    // SAFETY: the pool is built only from `&str` pushes, so every recorded range is valid UTF-8.
    unsafe { std::str::from_utf8_unchecked(&bytes[start as usize..end as usize]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reads_subranges() {
        let bytes = b"helloworld".as_slice();
        let src = StringSource::plain(bytes);
        assert_eq!(src.get(0..5), "hello");
        assert_eq!(src.get(5..10), "world");
        assert_eq!(src.materialize(), b"helloworld");
    }

    #[test]
    fn lazy_inflates_once_and_slices_at_base() {
        // An identity "codec" proves the OnceCell inflate + base/len slicing independent of any
        // real compression. `frame` stands in for two concatenated documents "AAA" + "BBBB".
        let frame = b"AAABBBB".as_slice();
        let buf = OnceCell::new();
        fn decode(f: &[u8]) -> Vec<u8> {
            f.to_vec()
        }

        // Second document: base 3, len 4.
        let state1 = LazyState {
            buf: &buf,
            frame,
            base: 3,
            len: 4,
            decode,
        };
        let doc1 = StringSource::lazy(&state1);
        assert!(buf.get().is_none(), "not inflated until first read");
        assert_eq!(doc1.get(0..4), "BBBB");
        assert!(buf.get().is_some(), "inflated on first read");
        assert_eq!(doc1.get(1..3), "BB");
        assert_eq!(doc1.materialize(), b"BBBB");

        // A second source sharing the same buffer reuses the already-inflated bytes.
        let state0 = LazyState {
            buf: &buf,
            frame,
            base: 0,
            len: 3,
            decode,
        };
        let doc0 = StringSource::lazy(&state0);
        assert_eq!(doc0.get(0..3), "AAA");
    }
}
