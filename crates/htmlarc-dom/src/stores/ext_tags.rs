use rkyv::{Archive, Deserialize, Serialize};

use crate::dom::NodeIndex;
use crate::html::HtmlTag;

use super::symbols::Sym;

/// First node-byte value that indexes the per-document extended-tag vocab (ADR 0002 §4).
/// Bytes `[0, EXT_BASE)` are `HtmlTag` discriminants; `[EXT_BASE, EXT_OVERFLOW)` index
/// [`ExtTags::vocab`]; `EXT_OVERFLOW` is the overflow sentinel resolved via the side map.
pub(crate) const EXT_BASE: u8 = 192;
pub(crate) const EXT_OVERFLOW: u8 = 255;
/// Distinct extended tag names a document can hold inline before spilling to the overflow
/// side map: `EXT_OVERFLOW - EXT_BASE` = 63.
const VOCAB_CAP: usize = (EXT_OVERFLOW - EXT_BASE) as usize;

// `HtmlTag::extended` is a normalization marker, never a stored byte; every real enum
// discriminant must stay below the vocab range so the two can never collide.
const _: () = assert!((HtmlTag::extended as u8) < EXT_BASE);

/// Per-document extended-tag store (ADR 0002 §4).
///
/// A node whose tag byte is `>= EXT_BASE` is a custom/unknown element; the byte resolves to
/// a [`Sym`] in the document symbol table (shared with class tokens and extended attribute
/// names) and from there to the real tag name. The first 63 distinct names are encoded
/// inline as `EXT_BASE + i` indexing `vocab`; any beyond that share the `EXT_OVERFLOW`
/// sentinel byte and are disambiguated by node index through `overflow` — a node-keyed side
/// map kept ascending by node index, so resolution is a binary search. Common Crawl's worst
/// document holds ~2,698 distinct extended tags, so the side map must scale to thousands.
///
/// Invariant: a [`Sym`] is encoded in **either** `vocab` **or** `overflow`, never both —
/// [`encode`](Self::encode) checks the vocab first, and once the vocab is full it never
/// admits a new sym. The selector fast path relies on this: a vocab tag is matched by node
/// tag-byte equality alone.
#[derive(Default, Hash, Archive, Serialize, Deserialize, Clone)]
pub struct ExtTags {
    /// `vocab[i]` is the symbol encoded as node byte `EXT_BASE + i` (≤ 63 entries).
    vocab: Vec<u16>,
    /// `(node index, symbol)` for nodes whose byte is `EXT_OVERFLOW`, ascending by node index.
    overflow: Vec<(u32, u16)>,
}

impl ExtTags {
    /// Encode a tag-name symbol for the node about to be created at `node`, returning its node
    /// byte. Reuses an existing vocab slot for a repeated name; assigns the next vocab slot
    /// while under [`VOCAB_CAP`]; otherwise records `(node, sym)` in the overflow side map and
    /// returns [`EXT_OVERFLOW`]. Shared verbatim by parse and rebuild, so the two always
    /// produce identical encodings.
    pub(crate) fn encode(&mut self, sym: Sym, node: NodeIndex) -> u8 {
        let sym = sym.as_u16();
        if let Some(i) = self.vocab.iter().position(|&s| s == sym) {
            return EXT_BASE + i as u8;
        }
        if self.vocab.len() < VOCAB_CAP {
            let byte = EXT_BASE + self.vocab.len() as u8;
            self.vocab.push(sym);
            byte
        } else {
            debug_assert!(
                self.overflow.last().is_none_or(|&(n, _)| n < node.as_u32()),
                "extended-tag overflow side map must stay ascending by node index"
            );
            self.overflow.push((node.as_u32(), sym));
            EXT_OVERFLOW
        }
    }

    pub(crate) fn view(&self) -> ExtTagsView<'_> {
        ExtTagsView {
            vocab: VocabView::Owned(&self.vocab),
            overflow: OverflowView::Owned(&self.overflow),
        }
    }
}

/// The vocab: owned native `u16`s, or archived little-endian ones read via `.to_native()`.
#[derive(Clone, Copy)]
enum VocabView<'a> {
    Owned(&'a [u16]),
    Archived(&'a [rkyv::Archived<u16>]),
}

impl VocabView<'_> {
    fn at(&self, index: usize) -> u16 {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => s[index].to_native(),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Owned(s) => s.len(),
            Self::Archived(s) => s.len(),
        }
    }

    /// The vocab slot holding `sym`, if any. A linear scan — the vocab is ≤ 63 entries.
    fn position(&self, sym: u16) -> Option<usize> {
        (0..self.len()).find(|&i| self.at(i) == sym)
    }
}

/// The overflow side map: owned native `(u32, u16)` pairs, or archived little-endian ones.
#[derive(Clone, Copy)]
enum OverflowView<'a> {
    Owned(&'a [(u32, u16)]),
    Archived(&'a [rkyv::Archived<(u32, u16)>]),
}

impl OverflowView<'_> {
    fn at(&self, index: usize) -> (u32, u16) {
        match self {
            Self::Owned(s) => s[index],
            Self::Archived(s) => (s[index].0.to_native(), s[index].1.to_native()),
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::Owned(s) => s.len(),
            Self::Archived(s) => s.len(),
        }
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Binary-search the side map for `node`'s symbol (entries are ascending by node index).
    fn find(&self, node: u32) -> Option<u16> {
        use std::cmp::Ordering::*;
        let mut left = 0usize;
        let mut right = self.len();
        while left < right {
            let mid = left + (right - left) / 2;
            let (n, sym) = self.at(mid);
            match n.cmp(&node) {
                Less => left = mid + 1,
                Greater => right = mid,
                Equal => return Some(sym),
            }
        }
        None
    }
}

/// Borrowed, read-only view over an [`ExtTags`] — owned or rkyv-archived.
#[derive(Clone, Copy)]
pub(crate) struct ExtTagsView<'a> {
    vocab: VocabView<'a>,
    overflow: OverflowView<'a>,
}

impl ExtTagsView<'_> {
    /// The tag-name symbol of a node whose byte is `>= EXT_BASE`.
    pub(crate) fn sym_at(&self, node: NodeIndex, byte: u8) -> Sym {
        debug_assert!(byte >= EXT_BASE);
        if byte < EXT_OVERFLOW {
            Sym(self.vocab.at((byte - EXT_BASE) as usize))
        } else {
            Sym(self
                .overflow
                .find(node.as_u32())
                .expect("extended-tag overflow byte must have a side-map entry"))
        }
    }

    /// The vocab byte a symbol is encoded as, or `None` if it is not a vocab tag (absent, or
    /// in the overflow map). Used by the selector resolve pass.
    pub(crate) fn vocab_byte(&self, sym: Sym) -> Option<u8> {
        self.vocab
            .position(sym.as_u16())
            .map(|i| EXT_BASE + i as u8)
    }

    /// Whether the overflow side map is empty — lets the resolve pass prune a non-vocab name
    /// to `Absent` instead of an overflow probe when no document tag overflowed.
    pub(crate) fn overflow_is_empty(&self) -> bool {
        self.overflow.is_empty()
    }
}

impl ArchivedExtTags {
    pub(crate) fn view(&self) -> ExtTagsView<'_> {
        ExtTagsView {
            vocab: VocabView::Archived(self.vocab.as_slice()),
            overflow: OverflowView::Archived(self.overflow.as_slice()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_dedups_repeated_names_into_one_vocab_slot() {
        let mut ext = ExtTags::default();
        let b0 = ext.encode(Sym(10), NodeIndex::new(1));
        let b1 = ext.encode(Sym(11), NodeIndex::new(2));
        let b0_again = ext.encode(Sym(10), NodeIndex::new(3));
        assert_eq!(b0, EXT_BASE);
        assert_eq!(b1, EXT_BASE + 1);
        assert_eq!(b0_again, EXT_BASE, "a repeated sym reuses its vocab slot");
        let view = ext.view();
        assert_eq!(view.sym_at(NodeIndex::new(1), b0), Sym(10));
        assert_eq!(view.sym_at(NodeIndex::new(2), b1), Sym(11));
    }

    #[test]
    fn vocab_fills_to_63_then_spills_to_overflow() {
        let mut ext = ExtTags::default();
        // 63 distinct syms fill the vocab (bytes 192..=254).
        for i in 0..VOCAB_CAP as u16 {
            let byte = ext.encode(Sym(i), NodeIndex::new(i as u32 + 1));
            assert_eq!(byte, EXT_BASE + i as u8);
        }
        // The 64th distinct sym overflows.
        let node = NodeIndex::new(1000);
        let byte = ext.encode(Sym(1000), node);
        assert_eq!(byte, EXT_OVERFLOW);
        let view = ext.view();
        assert_eq!(view.sym_at(node, byte), Sym(1000));
        // A 65th distinct sym also overflows, resolved by node index.
        let node2 = NodeIndex::new(2000);
        let byte2 = ext.encode(Sym(2000), node2);
        assert_eq!(byte2, EXT_OVERFLOW);
        assert_eq!(view_of(&ext).sym_at(node2, byte2), Sym(2000));
        assert_eq!(view_of(&ext).sym_at(node, EXT_OVERFLOW), Sym(1000));
    }

    fn view_of(ext: &ExtTags) -> ExtTagsView<'_> {
        ext.view()
    }

    #[test]
    fn vocab_byte_and_overflow_disjoint() {
        let mut ext = ExtTags::default();
        for i in 0..VOCAB_CAP as u16 {
            ext.encode(Sym(i), NodeIndex::new(i as u32 + 1));
        }
        let overflow_sym = Sym(500);
        ext.encode(overflow_sym, NodeIndex::new(5000));
        let view = ext.view();
        // A vocab sym resolves to a byte; the overflow sym does not (it is in the side map).
        assert_eq!(view.vocab_byte(Sym(0)), Some(EXT_BASE));
        assert_eq!(view.vocab_byte(Sym(62)), Some(EXT_OVERFLOW - 1));
        assert_eq!(view.vocab_byte(overflow_sym), None);
        assert!(!view.overflow_is_empty());
    }
}
