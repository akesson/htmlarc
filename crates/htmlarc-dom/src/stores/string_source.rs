use std::ops::Range;
use std::sync::OnceLock;

/// Inflates one compressed string block to its raw bytes. The archive layer implements this
/// (it owns the codec and any dictionary), so this crate stays codec-agnostic — it only ever
/// holds a `&dyn FrameDecoder`. `raw_len` is the decompressed length, letting the implementation
/// size the output buffer exactly. `Sync` so a decoder owned by a shared archive can be borrowed
/// from many reader threads at once.
pub trait FrameDecoder: Sync {
    fn decode(&self, frame: &[u8], raw_len: usize) -> Vec<u8>;
}

/// Borrowed, read-only source of a document's text/comment pool, the single seam through
/// which [`DomView`](crate::dom::DomView) reads text. It abstracts *how* the bytes are stored
/// so the query layer is agnostic to it:
///
/// - [`Plain`](Self::Plain): the bytes are available verbatim — an owned `Vec` (a live
///   document) or an already-inflated pool. Reads are zero-copy.
/// - [`Lazy`](Self::Lazy): the pool is split into independently compressed blocks; a read
///   inflates only the block containing its range, so a sweep that touches a fraction of a
///   document's text never pays to inflate the rest. Block boundaries coincide with text-node
///   boundaries (a write-side invariant), so a single node's range never straddles blocks and
///   reads stay borrowed slices. `decoder` is injected by the archive layer so this crate stays
///   codec-agnostic; the enum stays `Copy` because the whole [`LazyState`] sits behind one
///   reference.
///
/// All byte offsets handed to [`get`](Self::get) are document-local; for `Plain` the slice is
/// already narrowed to the document, and for `Lazy` `base` locates the document within the
/// bundle-cumulative block tables.
///
/// The per-block inflate state behind [`StringSource::Lazy`], held behind a shared reference so
/// the enum stays small (two words) — keeping the hot `Plain` path, and the `Copy`
/// [`DomView`](crate::dom::DomView) that carries it, as cheap as a bare slice.
///
/// The three slices describe this document's blocks: `bufs[i]` is block `i`'s decompression
/// cache (a synchronized [`OnceLock`], not a `OnceCell`, so read handles that embed one stay
/// `Sync`; on the hot path an already-initialized block costs a single atomic load), and the two
/// offset tables have one more entry than `bufs` (fencepost form). Offsets are *bundle*-absolute
/// so a whole bundle's tables can be sliced per document without rebasing copies:
/// `frame_starts[i]..frame_starts[i+1]` locates block `i`'s frame inside `frames` (the bundle's
/// concatenated frame blob), and `raw_starts[i]..raw_starts[i+1]` its inflated bytes, with
/// `base == raw_starts[0]` anchoring the document's local offsets.
pub struct LazyState<'a> {
    pub bufs: &'a [OnceLock<Vec<u8>>],
    pub frames: &'a [u8],
    pub frame_starts: &'a [u32],
    pub raw_starts: &'a [u32],
    pub base: u32,
    pub len: u32,
    pub decoder: &'a dyn FrameDecoder,
}

impl<'a> LazyState<'a> {
    /// Block `i` inflated, from its cache when already touched. The returned slice borrows the
    /// `OnceLock`'s buffer, so it lives as long as the caches themselves (`'a`), not this call.
    fn block(&self, i: usize) -> &'a [u8] {
        self.bufs[i].get_or_init(|| {
            let frame =
                &self.frames[self.frame_starts[i] as usize..self.frame_starts[i + 1] as usize];
            let raw_len = (self.raw_starts[i + 1] - self.raw_starts[i]) as usize;
            self.decoder.decode(frame, raw_len)
        })
    }
}

#[derive(Clone, Copy)]
pub enum StringSource<'a> {
    Plain(&'a [u8]),
    Lazy(&'a LazyState<'a>),
}

impl<'a> StringSource<'a> {
    /// A source over verbatim bytes (owned `Vec` or an already-inflated pool).
    pub fn plain(bytes: &'a [u8]) -> Self {
        StringSource::Plain(bytes)
    }

    /// A lazily-inflated source over a document's compressed blocks (the per-document view of a
    /// bundle's shared tables — see [`LazyState`]).
    pub fn lazy(state: &'a LazyState<'a>) -> Self {
        StringSource::Lazy(state)
    }

    /// The text for `range` (document-local byte offsets). UTF-8 is an invariant of the stored
    /// pool, so the slice is decoded unchecked — identical to the previous flat-pool accessor.
    pub(crate) fn get(&self, range: Range<u32>) -> &'a str {
        match *self {
            StringSource::Plain(bytes) => unchecked_str(bytes, range.start, range.end),
            StringSource::Lazy(s) => {
                // Empty ranges first: fresh nodes carry `0..0`, `replace_text(_, "")` can record
                // `len..len`, and a text-free document has no blocks to search at all.
                if range.start == range.end {
                    return "";
                }
                debug_assert!(range.end <= s.len, "range past this document's segment");
                let start = s.base + range.start;
                let i = if s.bufs.len() == 1 {
                    0
                } else {
                    // Last block whose start is at or before `start`. The terminal fencepost
                    // (`raw_starts[n] == base + len`) is strictly greater than `start` for any
                    // non-empty in-bounds range, so the result is always a real block.
                    s.raw_starts.partition_point(|&s0| s0 <= start) - 1
                };
                debug_assert!(
                    s.base + range.end <= s.raw_starts[i + 1],
                    "text range straddles a block boundary"
                );
                unchecked_str(
                    s.block(i),
                    start - s.raw_starts[i],
                    s.base + range.end - s.raw_starts[i],
                )
            }
        }
    }

    /// Copy out this document's entire text segment, inflating every block. Used when an
    /// archived document is materialised into an owned, editable
    /// [`DomInner`](crate::dom::DomInner) (the archived→owned `repackage` path), which needs a
    /// fresh owned pool. Blocks partition the segment exactly (they never straddle documents),
    /// so this is a straight concatenation.
    pub fn materialize(&self) -> Vec<u8> {
        match *self {
            StringSource::Plain(bytes) => bytes.to_vec(),
            StringSource::Lazy(s) => {
                let mut pool = Vec::with_capacity(s.len as usize);
                for i in 0..s.bufs.len() {
                    pool.extend_from_slice(s.block(i));
                }
                pool
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

    /// An identity "codec" — proves the per-block `OnceLock` inflate and table slicing
    /// independent of any real compression (each block's "frame" is its raw bytes).
    struct Identity;
    impl FrameDecoder for Identity {
        fn decode(&self, frame: &[u8], _raw_len: usize) -> Vec<u8> {
            frame.to_vec()
        }
    }

    #[test]
    fn lazy_inflates_per_block_and_slices() {
        // One document split into two blocks: "AAA" + "BBBB".
        let frames = b"AAABBBB".as_slice();
        let bufs = [OnceLock::new(), OnceLock::new()];
        let decoder = Identity;
        let state = LazyState {
            bufs: &bufs,
            frames,
            frame_starts: &[0, 3, 7],
            raw_starts: &[0, 3, 7],
            base: 0,
            len: 7,
            decoder: &decoder,
        };
        let doc = StringSource::lazy(&state);

        assert!(bufs[0].get().is_none() && bufs[1].get().is_none());
        assert_eq!(doc.get(4..6), "BB");
        assert!(bufs[0].get().is_none(), "untouched block stays cold");
        assert!(bufs[1].get().is_some(), "touched block inflates");
        assert_eq!(
            doc.get(3..7),
            "BBBB",
            "range starting exactly at a block start"
        );
        assert_eq!(doc.get(0..3), "AAA");
        assert!(bufs[0].get().is_some());

        // Empty ranges never touch a block, wherever they sit.
        assert_eq!(doc.get(0..0), "");
        assert_eq!(
            doc.get(7..7),
            "",
            "terminal empty range (replace_text(_, \"\"))"
        );

        assert_eq!(doc.materialize(), b"AAABBBB");
    }

    #[test]
    fn lazy_zero_block_document() {
        let bufs: [OnceLock<Vec<u8>>; 0] = [];
        let decoder = Identity;
        let state = LazyState {
            bufs: &bufs,
            frames: b"",
            frame_starts: &[0],
            raw_starts: &[0],
            base: 0,
            len: 0,
            decoder: &decoder,
        };
        let doc = StringSource::lazy(&state);
        assert_eq!(doc.get(0..0), "");
        assert_eq!(doc.materialize(), b"");
    }

    #[test]
    fn lazy_slices_bundle_absolute_tables_at_base() {
        // Two documents sharing one bundle's tables: doc0 = "AAA" (block 0), doc1 = "BBBB"
        // (block 1). Each LazyState is a per-document subslice, offsets stay bundle-absolute.
        let frames = b"AAABBBB".as_slice();
        let bufs = [OnceLock::new(), OnceLock::new()];
        let frame_starts = [0u32, 3, 7];
        let raw_starts = [0u32, 3, 7];
        let decoder = Identity;

        let doc1_state = LazyState {
            bufs: &bufs[1..2],
            frames,
            frame_starts: &frame_starts[1..3],
            raw_starts: &raw_starts[1..3],
            base: 3,
            len: 4,
            decoder: &decoder,
        };
        let doc1 = StringSource::lazy(&doc1_state);
        assert_eq!(doc1.get(0..4), "BBBB");
        assert_eq!(doc1.get(1..3), "BB");
        assert!(bufs[0].get().is_none(), "doc0's block untouched");
        assert_eq!(doc1.materialize(), b"BBBB");

        let doc0_state = LazyState {
            bufs: &bufs[0..1],
            frames,
            frame_starts: &frame_starts[0..2],
            raw_starts: &raw_starts[0..2],
            base: 0,
            len: 3,
            decoder: &decoder,
        };
        let doc0 = StringSource::lazy(&doc0_state);
        assert_eq!(doc0.get(0..3), "AAA");
    }
}
