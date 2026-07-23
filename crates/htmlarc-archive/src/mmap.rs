use std::fmt::Debug;
use std::ops::{Index, Range};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use htmlarc_dom::prelude::{
    ArchivedDom, ContiguousDfs, DomInner, DomRead, DomRef, DomView, FrameDecoder, HtmlElement,
    LazyState, LinearSweep, NodeIndex, NodesView, StringSource,
};
use memmap2::Mmap;
use rkyv::rancor::Error;

use crate::Filter;
use crate::bundle::ArchivedBundleTable;
use crate::bundle_strings::{ArchivedBundleStrings, DocBlocks};
use crate::codec::ZstdFrameDecoder;
use crate::doc_table::{self, ArchivedDocTable, ArchivedSortIndex};
use crate::entry::ArchivedHtmlEntry;
use crate::error::ArchiveErr;
use crate::trailer::Trailer;

/// A memory-mapped v4 `.htmlarc` archive, queried **lazily and zero-copy**: opening maps the file
/// and validates only the footer (trailer + doc table + sort index + bundle table); each
/// document's blob is validated and read in place the moment it is fetched — never deserialized,
/// never all faulted in at once.
///
/// Metadata (`len`, `keys`, checksum-by-key, bundle ranges) is served straight from the footer
/// without touching any blob. Bulk iteration proceeds **bundle→doc** (doc-table order); keyed
/// lookup binary-searches the sort-index permutation.
///
/// # Caveat
/// The file backing the mapping must not be truncated or rewritten in place while an
/// `MmapArchive` is open — doing so can raise `SIGBUS` on access. [`crate::ArchiveWriter`] writes
/// to a temp path and renames, so it never corrupts a mapping of a previous build.
pub struct MmapArchive {
    mmap: Mmap,
    doc_table_offset: usize,
    doc_table_len: usize,
    bundle_table_offset: usize,
    bundle_table_len: usize,
    sort_index_offset: usize,
    sort_index_len: usize,
    /// Inflates per-document string frames, holding the archive-wide dictionary (if any). Built
    /// once at open and shared (by `&`) with every read scope — it owns no per-read state, so the
    /// archive stays immutable and `Sync`.
    decoder: ZstdFrameDecoder,
    /// Memoizes which bundles' string blocks have already passed full validation. The block is
    /// faulted in lazily — only when a document in its bundle is first read — at which point
    /// [`bundle_strings`](Self::bundle_strings) validates it (safe rkyv `access` + offset-table
    /// walk). Because the mapping is immutable (see the type-level caveat), a once-valid block
    /// stays valid, so every later fetch skips straight to `access_unchecked`. This turns a sweep
    /// that touches all ~`BUNDLE_CAP` documents of a bundle — each re-fetching the shared block —
    /// from one revalidation *per document* (quadratic in bundle size) into one *per bundle*. It is
    /// pure memoization (observationally immutable), so the archive stays `Sync`.
    bundles_validated: Box<[AtomicBool]>,
    /// The typed metadata table's region (ADR 0009), `None` when the archive carries no
    /// metadata. Fully validated at open ([`crate::meta::validate_archived`]) — metadata is
    /// tiny relative to the archive — so reads go straight to `access_unchecked`.
    meta_region: Option<(usize, usize)>,
}

impl MmapArchive {
    /// Memory-map a v4 `.htmlarc` and validate its footer (trailer bounds + the doc table, sort
    /// index, and bundle table). Individual document blobs are validated lazily on access.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ArchiveErr> {
        let file = std::fs::File::open(path.as_ref()).map_err(ArchiveErr::FileRead)?;
        // SAFETY: the mapping is only ever read; the caller must not mutate the file while it is
        // open (see the type-level caveat).
        let mmap = unsafe { Mmap::map(&file) }.map_err(ArchiveErr::FileRead)?;

        crate::header::validate_header(&mmap)?;
        let trailer = Trailer::read_from_tail(&mmap)?;
        let doc_table_offset = trailer.doc_table_offset as usize;
        let doc_table_len = trailer.doc_table_len as usize;
        let bundle_table_offset = trailer.bundle_table_offset as usize;
        let bundle_table_len = trailer.bundle_table_len as usize;
        let sort_index_offset = trailer.sort_index_offset as usize;
        let sort_index_len = trailer.sort_index_len as usize;

        // Validate the doc table first (the sort index dereferences into it), then the sort index,
        // then the bundle table — safe rkyv access (bytecheck). A corrupt/misaligned footer
        // becomes an `Err` here rather than UB on the first query.
        let doc_slice = slice(&mmap, doc_table_offset, doc_table_len, "doc table")?;
        let doc_table = rkyv::access::<ArchivedDocTable, Error>(doc_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;
        let doc_count = doc_table.len();

        let sort_slice = slice(&mmap, sort_index_offset, sort_index_len, "sort index")?;
        let sort_index = rkyv::access::<ArchivedSortIndex, Error>(sort_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        // bytecheck validates structure, not values: a corrupt permutation entry would index
        // out of the doc table and panic at query time. Reject it up front.
        if sort_index.len() != doc_count {
            return Err(ArchiveErr::Validate(
                "sort index length does not match the doc table".into(),
            ));
        }
        for p in sort_index.iter() {
            if (p.to_native() as usize) >= doc_count {
                return Err(ArchiveErr::Validate(
                    "sort index entry out of range for the doc table".into(),
                ));
            }
        }

        let bt_slice = slice(&mmap, bundle_table_offset, bundle_table_len, "bundle table")?;
        let bundle_table = rkyv::access::<ArchivedBundleTable, Error>(bt_slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;

        // Bundles must tile the doc table contiguously: a malformed table would let a bundle
        // range index out of bounds. Verify it covers [0, doc_count) exactly.
        let mut expected_start = 0usize;
        for desc in bundle_table.iter() {
            if desc.doc_start.to_native() as usize != expected_start {
                return Err(ArchiveErr::Validate(
                    "bundle table is not contiguous over the doc table".into(),
                ));
            }
            expected_start += desc.doc_count.to_native() as usize;
        }
        if expected_start != doc_count {
            return Err(ArchiveErr::Validate(
                "bundle table does not cover the whole doc table".into(),
            ));
        }

        // The archive-wide string-compression dictionary (ADR 0005), if one was trained. A
        // zero-length region means the per-document frames were compressed dictionary-less.
        let dict = if trailer.dict_len > 0 {
            let s = slice(
                &mmap,
                trailer.dict_offset as usize,
                trailer.dict_len as usize,
                "dictionary",
            )?;
            Some(s.to_vec())
        } else {
            None
        };
        let decoder = ZstdFrameDecoder::new(dict);

        // One validation flag per bundle (cleared); each bundle's string block is validated on its
        // first fetch and then read unchecked (see `bundles_validated`).
        let bundles_validated = (0..bundle_table.len())
            .map(|_| AtomicBool::new(false))
            .collect();

        // The typed metadata table (ADR 0009), if present — validated fully here (structure via
        // safe rkyv access, values via `validate_archived`), so per-row reads never re-check.
        let meta_region = if trailer.meta_len > 0 {
            let off = trailer.meta_offset as usize;
            let len = trailer.meta_len as usize;
            let s = slice(&mmap, off, len, "metadata table")?;
            let table = rkyv::access::<crate::meta::ArchivedMetaTable, Error>(s)
                .map_err(|e| ArchiveErr::Validate(e.to_string()))?;
            crate::meta::validate_archived(table, doc_count)?;
            Some((off, len))
        } else {
            None
        };

        Ok(Self {
            mmap,
            doc_table_offset,
            doc_table_len,
            bundle_table_offset,
            bundle_table_len,
            sort_index_offset,
            sort_index_len,
            decoder,
            bundles_validated,
            meta_region,
        })
    }

    /// The archive's typed metadata table (ADR 0009), or `None` when it carries no metadata.
    /// Row `i` belongs to the document at doc-table position `i`.
    pub fn meta(&self) -> Option<&crate::meta::ArchivedMetaTable> {
        let (off, len) = self.meta_region?;
        let s = &self.mmap[off..off + len];
        // SAFETY: `open` validated this exact slice and the mapping is immutable.
        Some(unsafe { rkyv::access_unchecked::<crate::meta::ArchivedMetaTable>(s) })
    }

    /// The declared metadata schema, or `None` when the archive carries no metadata.
    pub fn meta_schema(&self) -> Option<crate::meta::MetaSchema> {
        self.meta().map(|t| t.schema())
    }

    /// The metadata value of field `field` for the document at doc-table position `pos`
    /// (`None` = null). Panics if the archive has no metadata or the indices are out of
    /// range — callers gate on [`meta`](Self::meta)/[`meta_schema`](Self::meta_schema).
    pub fn meta_value(&self, pos: usize, field: usize) -> Option<crate::meta::MetaRef<'_>> {
        let table = self.meta().expect("archive has no metadata table");
        crate::meta::archived_value(&table.columns[field], pos)
    }

    fn doc_table(&self) -> &ArchivedDocTable {
        // SAFETY: `open` validated this exact slice and the mapping is immutable.
        let s = &self.mmap[self.doc_table_offset..self.doc_table_offset + self.doc_table_len];
        unsafe { rkyv::access_unchecked::<ArchivedDocTable>(s) }
    }

    fn sort_index(&self) -> &ArchivedSortIndex {
        let s = &self.mmap[self.sort_index_offset..self.sort_index_offset + self.sort_index_len];
        unsafe { rkyv::access_unchecked::<ArchivedSortIndex>(s) }
    }

    fn bundle_table(&self) -> &ArchivedBundleTable {
        let s =
            &self.mmap[self.bundle_table_offset..self.bundle_table_offset + self.bundle_table_len];
        unsafe { rkyv::access_unchecked::<ArchivedBundleTable>(s) }
    }

    /// Validate and read the doc blob at `(offset, len)` in place. Uses *safe* `access` so a
    /// corrupt blob surfaces as an `Err` instead of UB.
    fn blob(&self, offset: u64, len: u64) -> Result<&ArchivedHtmlEntry, ArchiveErr> {
        let end = offset
            .checked_add(len)
            .ok_or_else(|| ArchiveErr::Validate("blob offset/len overflow".into()))?;
        let slice = self
            .mmap
            .get(offset as usize..end as usize)
            .ok_or_else(|| ArchiveErr::Validate("document blob lies outside the file".into()))?;
        rkyv::access::<ArchivedHtmlEntry, Error>(slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))
    }

    pub fn len(&self) -> usize {
        self.doc_table().len()
    }

    pub fn is_empty(&self) -> bool {
        self.doc_table().is_empty()
    }

    /// Number of bundles.
    pub fn bundle_count(&self) -> usize {
        self.bundle_table().len()
    }

    /// The half-open flat (doc-table) position range covered by bundle `b`.
    pub fn bundle_range(&self, b: usize) -> Range<usize> {
        let desc = &self.bundle_table()[b];
        let start = desc.doc_start.to_native() as usize;
        start..start + desc.doc_count.to_native() as usize
    }

    /// The bundle that owns flat (doc-table) position `i`. Bundles tile `[0, len)` contiguously by
    /// `doc_start`, so this is the last bundle whose start is `<= i`.
    fn bundle_of(&self, i: usize) -> usize {
        self.bundle_table()
            .partition_point(|d| (d.doc_start.to_native() as usize) <= i)
            - 1
    }

    /// Validate and read bundle `b`'s relocated string block in place (safe `access`, plus an
    /// offset-table sanity check), the way `blob` does for document blobs — lazily, only when a
    /// document in that bundle is actually read.
    ///
    /// The full validation runs **once per bundle**: the first fetch validates the block and records
    /// it in `bundles_validated`; every later fetch (e.g. a sweep that reads each of the bundle's
    /// ~`BUNDLE_CAP` documents) skips the bytecheck + offset-table walk and casts in place via
    /// `access_unchecked` — sound because the mapping is immutable, so a once-valid block stays
    /// valid. This is what stops a per-document fetch loop from re-validating the shared block
    /// quadratically. `bundle_strings` stays the single validation gate either way.
    pub fn bundle_strings(&self, b: usize) -> Result<&ArchivedBundleStrings, ArchiveErr> {
        let desc = &self.bundle_table()[b];
        let off = desc.data_offset.to_native();
        let len = desc.data_len.to_native();
        let end = off
            .checked_add(len)
            .ok_or_else(|| ArchiveErr::Validate("bundle data offset/len overflow".into()))?;
        let slice = self.mmap.get(off as usize..end as usize).ok_or_else(|| {
            ArchiveErr::Validate("bundle string block lies outside the file".into())
        })?;

        // Fast path: this block already passed the checks below on an earlier fetch. The mapping is
        // immutable, so it is still valid — skip straight to the unchecked cast.
        if self.bundles_validated[b].load(Ordering::Acquire) {
            // SAFETY: validated by the slow path before the flag was set; the bytes cannot change.
            return Ok(unsafe { rkyv::access_unchecked::<ArchivedBundleStrings>(slice) });
        }

        let bs = rkyv::access::<ArchivedBundleStrings, Error>(slice)
            .map_err(|e| ArchiveErr::Validate(e.to_string()))?;
        if !bs.validate() {
            return Err(ArchiveErr::Validate(
                "bundle string block has a malformed offset table".into(),
            ));
        }
        // Cross-check the block against the bundle descriptor (the owned load path does the same):
        // if the two disagree, a later `frame(slot)`/`lazy_states` could index past the offset
        // table and panic, so surface the mismatch as `Err` here instead — keeping
        // `bundle_strings` the one validation gate for the block.
        if bs.doc_count() != desc.doc_count.to_native() as usize {
            return Err(ArchiveErr::Validate(
                "bundle string block document count disagrees with its bundle descriptor".into(),
            ));
        }
        // Only a fully validated block sets the flag, so a corrupt block re-runs (and re-rejects)
        // these checks on every fetch rather than being cached as good.
        self.bundles_validated[b].store(true, Ordering::Release);
        Ok(bs)
    }

    /// The [`FrameDecoder`] for this archive's string frames (holds the archive-wide dictionary,
    /// if any). Sweeps pass it to [`ArchivedBundleStrings::lazy_states`].
    pub fn decoder(&self) -> &dyn FrameDecoder {
        &self.decoder
    }

    /// Validate the document at flat position `i` and pair it with its bundle's relocated text,
    /// ready to query/render. `Err` if either the document blob or the bundle's string block fails
    /// validation. The returned [`Doc`] holds the document's compressed block frames and inflates
    /// them lazily, per block, so a query that never reads text never decompresses — and one that
    /// reads a sliver inflates only the blocks it touches.
    fn try_doc(&self, i: usize) -> Result<Doc<'_>, ArchiveErr> {
        let b = self.bundle_of(i);
        let slot = i - self.bundle_table()[b].doc_start.to_native() as usize;
        let d = &self.doc_table()[i];
        let entry = self.blob(d.offset.to_native(), d.len.to_native())?;
        let strings = self.bundle_strings(b)?;
        let blocks = strings.doc_blocks(slot);
        Ok(Doc {
            entry,
            frames: strings.frames(),
            bufs: blocks.bufs(),
            blocks,
            decoder: &self.decoder,
            full: OnceLock::new(),
        })
    }

    /// The queryable document at flat (bundle→doc) position `i`, paired with its bundle's text.
    /// Panics on a corrupt blob — like [`Index`], a bad blob at a valid index means the archive
    /// is corrupt and `Index` cannot return a `Result`. Use [`get`](Self::get)/[`key_at`](Self::key_at) for
    /// metadata-only access that never touches a blob.
    ///
    /// `i`'s bundle string block is validated only on its first fetch and read unchecked
    /// thereafter (see [`bundle_strings`](Self::bundle_strings)), so a loop that sweeps a whole
    /// bundle through `doc()` no longer revalidates the block per document. It still resolves the
    /// bundle (a binary search) and sets up a fresh inflate cache on each call, so for a large
    /// sweep prefer reading the block once with [`bundle_strings`](Self::bundle_strings) over a
    /// [`bundle_range`](Self::bundle_range) and binding each entry through
    /// [`lazy_states`](ArchivedBundleStrings::lazy_states) (as `entries_matching` and the `probe`
    /// sweep do).
    pub fn doc(&self, i: usize) -> Doc<'_> {
        self.try_doc(i).expect("corrupt document or bundle blob")
    }

    /// Look a document up by key and pair it with its text. `Ok(None)` = absent; `Err` = the
    /// matching blob (document or bundle strings) failed validation.
    pub fn doc_by_key(&self, key: &str) -> Result<Option<Doc<'_>>, ArchiveErr> {
        match doc_table::find_index(self.doc_table(), self.sort_index(), key) {
            Some(i) => Ok(Some(self.try_doc(i)?)),
            None => Ok(None),
        }
    }

    /// Look an entry up by key. `Ok(None)` = absent; `Err` = the matching blob failed validation.
    /// The binary search only reads the footer; the blob is touched only on a hit.
    pub fn get(&self, key: &str) -> Result<Option<&ArchivedHtmlEntry>, ArchiveErr> {
        match doc_table::find(self.doc_table(), self.sort_index(), key) {
            Some(d) => Ok(Some(self.blob(d.offset.to_native(), d.len.to_native())?)),
            None => Ok(None),
        }
    }

    /// The checksum for `key`, read straight from the doc table (no blob access).
    pub fn checksum_for_key(&self, key: &str) -> Option<u64> {
        doc_table::find(self.doc_table(), self.sort_index(), key).map(|d| d.checksum.to_native())
    }

    /// The flat (bundle→doc) position of `key`, via the sort index — footer-only, no blob access.
    /// Lets a keyed search resolve a word-list straight to positions instead of scanning.
    pub fn position_for_key(&self, key: &str) -> Option<usize> {
        doc_table::find_index(self.doc_table(), self.sort_index(), key)
    }

    /// The key at positional index `i` (bundle→doc order), no blob access.
    pub fn key_at(&self, i: usize) -> &str {
        self.doc_table()[i].key.as_str()
    }

    /// The checksum at positional index `i` (bundle→doc order), no blob access.
    pub fn checksum_at(&self, i: usize) -> u64 {
        self.doc_table()[i].checksum.to_native()
    }

    pub fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        self.doc_table().iter().map(move |d| {
            self.blob(d.offset.to_native(), d.len.to_native())
                .expect("corrupt document blob during iteration")
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.doc_table().iter().map(|d| d.key.as_str())
    }
}

/// Slice a region with a bounds check, turning an out-of-range footer offset into an `Err`.
fn slice<'a>(
    mmap: &'a [u8],
    offset: usize,
    len: usize,
    what: &str,
) -> Result<&'a [u8], ArchiveErr> {
    mmap.get(offset..offset + len)
        .ok_or_else(|| ArchiveErr::Validate(format!("{what} range lies outside the file")))
}

impl Index<usize> for MmapArchive {
    type Output = ArchivedHtmlEntry;

    /// Positional access (bundle→doc order). Panics on a corrupt blob — `Index` cannot return a
    /// `Result`, and a bad blob at a valid index means the archive is corrupt.
    fn index(&self, index: usize) -> &Self::Output {
        let d = &self.doc_table()[index];
        self.blob(d.offset.to_native(), d.len.to_native())
            .expect("corrupt document blob")
    }
}

impl crate::Archive for MmapArchive {
    type Entry = ArchivedHtmlEntry;

    fn len(&self) -> usize {
        self.doc_table().len()
    }

    fn get(&self, key: &str) -> Result<Option<&ArchivedHtmlEntry>, ArchiveErr> {
        MmapArchive::get(self, key)
    }

    fn entries(&self) -> impl Iterator<Item = &ArchivedHtmlEntry> {
        MmapArchive::entries(self)
    }

    fn keys(&self) -> impl Iterator<Item = &str> {
        MmapArchive::keys(self)
    }

    /// Sweep bundle→doc, binding each document to its bundle's text only when the filter's CSS
    /// rules actually need the body (a key-only filter short-circuits via [`Filter::keep_key`]).
    /// Reads the bundle string block once per bundle.
    fn entries_matching<'a>(&'a self, filter: &'a Filter) -> Vec<&'a str> {
        let mut keys = Vec::new();
        for b in 0..self.bundle_count() {
            let range = self.bundle_range(b);
            // Build the bundle's lazy text bindings only if some document needs the DOM inspected
            // (a key-only filter short-circuits every key). `arena` and `states` are two separate
            // locals so `states` can borrow `arena` without a self-referential struct; binding
            // stays lazy, so a document whose CSS rules never read text never inflates a block.
            let needs_body = range
                .clone()
                .any(|i| filter.keep_key(self.key_at(i)).is_none());
            let arena = needs_body.then(|| {
                let block = self.bundle_strings(b).expect("corrupt bundle string block");
                (block, block.arena())
            });
            let states = arena
                .as_ref()
                .map(|(block, arena)| block.lazy_states(self.decoder(), arena));
            for i in range.clone() {
                let key = self.key_at(i);
                let keep = match filter.keep_key(key) {
                    Some(k) => k,
                    None => {
                        let state = &states.as_ref().expect("arena built when a body is needed")
                            [i - range.start];
                        filter.keep(key, &self[i].bind(StringSource::lazy(state)))
                    }
                };
                if keep {
                    keys.push(key);
                }
            }
        }
        keys
    }
}

/// A single memory-mapped document paired with its bundle's relocated text, returned by
/// [`MmapArchive::doc`]/[`doc_by_key`](MmapArchive::doc_by_key). It owns one inflate cache per
/// text block and holds the document's *compressed* block frames, decompressing each at most
/// once — and only if text inside it is actually read — so a topology- or selector-only query
/// over a single document pays nothing for compression, and a partial text read pays only for
/// its blocks. Implements [`DomRead`], so it queries exactly like the underlying archived
/// document (`doc.root()`, `doc.to_html(..)`, `filter.keep(key, &doc)`).
///
/// For sweeping a whole bundle, prefer reading the block once via
/// [`MmapArchive::bundle_strings`] + [`ArchivedBundleStrings::lazy_states`]; `doc()` re-resolves the
/// bundle and sets up fresh inflate caches on every call (though it no longer re-validates the
/// block — that happens once per bundle).
pub struct Doc<'a> {
    entry: &'a ArchivedHtmlEntry,
    /// The bundle's whole frame blob; `blocks`' tables index into it (bundle-absolute).
    frames: &'a [u8],
    /// This document's block tables, copied to native `u32` at construction.
    blocks: DocBlocks,
    /// One inflate cache per block.
    bufs: Box<[OnceLock<Vec<u8>>]>,
    decoder: &'a dyn FrameDecoder,
    /// [`DomRef::dom_view`]'s contiguous whole-pool cache — a borrowed view needs one flat
    /// `Plain` slice, which the per-block caches cannot provide. A handle read through both
    /// paths holds its text twice; in exchange neither path pays for the other.
    full: OnceLock<Vec<u8>>,
}

impl<'a> Doc<'a> {
    /// The per-call [`LazyState`] over this document's blocks — all borrows, so it is built on
    /// the stack per call, keeping [`Doc`] free of a self-referential field. Repeated reads
    /// still inflate each block only once (the caches live on the handle).
    fn lazy_state(&self) -> LazyState<'_> {
        LazyState {
            bufs: &self.bufs,
            frames: self.frames,
            frame_starts: &self.blocks.frame_starts,
            raw_starts: &self.blocks.raw_starts,
            base: self.blocks.base(),
            len: self.blocks.raw_len(),
            decoder: self.decoder,
        }
    }

    /// Run `f` with a query view over this document.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn with_dom<R>(&self, f: impl FnOnce(&ArchivedDom<'_>) -> R) -> R {
        let state = self.lazy_state();
        let dom = self.entry.bind(StringSource::lazy(&state));
        f(&dom)
    }
}

impl Debug for Doc<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Doc").finish_non_exhaustive()
    }
}

impl DomRead for Doc<'_> {
    const IS_IMMUTABLE: bool = true;

    // The mmap'd archive blob is always contiguous DFS (stored as `into_optimal_width()` of parse
    // order, never mutated), so the mmap read path uses the layout-exploiting linear sweep.
    type Forward<'a>
        = LinearSweep<'a, Self>
    where
        Self: 'a;
    type Descendants<'a>
        = LinearSweep<'a, Self>
    where
        Self: 'a;

    fn forward_from(&self, start: NodeIndex) -> Self::Forward<'_> {
        LinearSweep::forwards_at(self, start)
    }

    fn descendants_from(&self, start: NodeIndex) -> Self::Descendants<'_> {
        LinearSweep::descendants_at(self, start)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        self.with_dom(|dom| dom.with_view(f))
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn with_nodes<F: FnOnce(NodesView<'_>) -> R, R>(&self, f: F) -> R {
        self.with_dom(|dom| dom.with_nodes(f))
    }

    fn walk_view(&self) -> Option<DomView<'_>> {
        // A view bound to the archive lifetime (which outlives `&self`), reused across the whole
        // select walk instead of rebinding per accessor (ADR 0007). The text source is empty:
        // resolved topology/attribute matching is integer-only and reads the blob's own stores
        // (nodes, symbols, attr values), never the relocated text pool, so no frame is inflated.
        // Text selectors (`[text]`/`[text*=]`) fall back to the per-call `with_view` path, which
        // still resolves text through the lazy source.
        Some(self.entry.bind(StringSource::plain(&[])).view())
    }

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::root(self)
    }

    fn repackage(&self) -> DomInner {
        self.with_dom(|dom| dom.repackage())
    }
}

impl DomRef for Doc<'_> {
    /// A view bound to the full `&self` borrow (what text accessors that return borrowed `&str`,
    /// and the probe's node analysis, need). Unlike the lazy [`with_view`](DomRead::with_view)
    /// path, this materialises the document's whole pool once into the owned `full` cache (a
    /// borrowed view cannot hand back transient inflate buffers, and `Plain` needs one flat
    /// slice), then reads it zero-copy.
    fn dom_view(&self) -> DomView<'_> {
        let text = self
            .full
            .get_or_init(|| StringSource::lazy(&self.lazy_state()).materialize());
        self.entry.bind(StringSource::plain(text)).view()
    }
}

impl ContiguousDfs for Doc<'_> {}

/// An owned, `'static` counterpart of [`Doc`]: the same lazily-inflated, queryable document, but
/// holding its archive by [`Arc`] instead of by borrow — so it can be stored in structs, moved
/// across threads, and handed to bindings that cannot carry lifetimes (Python, async servers).
///
/// Construction resolves the document's bundle and validates its blob and the bundle's string
/// block once (safe rkyv `access`); every later read casts in place unchecked — sound because the
/// mapping is immutable (see the [`MmapArchive`] caveat). The inflate caches live on the handle,
/// so however many text reads the handle serves, each of its string blocks decompresses at most
/// once — the two costs a per-call `archive.doc(i)` loop would otherwise re-pay per call.
pub struct OwnedDoc {
    archive: Arc<MmapArchive>,
    /// Flat (bundle→doc) position in the doc table.
    pos: usize,
    /// `pos`'s owning bundle, resolved once (the block tables below already narrow the bundle's
    /// string block to this document, so the slot itself is not kept).
    bundle: usize,
    /// The document blob's byte range, bytecheck-validated at construction.
    offset: usize,
    len: usize,
    /// This document's block tables, copied to native `u32` at construction.
    blocks: DocBlocks,
    /// Handle-lifetime inflate caches, one per block (see [`LazyState`]).
    bufs: Box<[OnceLock<Vec<u8>>]>,
    /// [`DomRef::dom_view`]'s contiguous whole-pool cache — see [`Doc::full`].
    full: OnceLock<Vec<u8>>,
}

impl OwnedDoc {
    /// The document at flat (bundle→doc) position `pos`, validated and ready to query. `Err` if
    /// the document blob or its bundle's string block fails validation. Panics if `pos` is out of
    /// range, like every positional accessor ([`MmapArchive::doc`], [`MmapArchive::key_at`]).
    pub fn new(archive: Arc<MmapArchive>, pos: usize) -> Result<Self, ArchiveErr> {
        let bundle = archive.bundle_of(pos);
        let slot = pos - archive.bundle_table()[bundle].doc_start.to_native() as usize;
        let d = &archive.doc_table()[pos];
        let (offset, len) = (d.offset.to_native(), d.len.to_native());
        // Validate the blob and the bundle's string block up front; the per-read accessors
        // ([`entry`](Self::entry), [`frame`](Self::frame)) rely on it to go unchecked.
        archive.blob(offset, len)?;
        let blocks = archive.bundle_strings(bundle)?.doc_blocks(slot);
        Ok(Self {
            pos,
            bundle,
            offset: offset as usize,
            len: len as usize,
            bufs: blocks.bufs(),
            blocks,
            full: OnceLock::new(),
            archive,
        })
    }

    /// Look the document up by key. `Ok(None)` = absent; `Err` = the matching blob (document or
    /// bundle strings) failed validation. The owned sibling of [`MmapArchive::doc_by_key`].
    pub fn by_key(archive: Arc<MmapArchive>, key: &str) -> Result<Option<Self>, ArchiveErr> {
        match doc_table::find_index(archive.doc_table(), archive.sort_index(), key) {
            Some(pos) => Self::new(archive, pos).map(Some),
            None => Ok(None),
        }
    }

    /// The entry key (e.g. the source file name).
    pub fn key(&self) -> &str {
        self.entry().key()
    }

    /// Checksum of the stored DOM, for fast archive diffing.
    pub fn checksum(&self) -> u64 {
        self.entry().checksum()
    }

    /// This document's flat (bundle→doc) position in the archive.
    pub fn position(&self) -> usize {
        self.pos
    }

    /// The archive this handle keeps alive.
    pub fn archive(&self) -> &Arc<MmapArchive> {
        &self.archive
    }

    fn entry(&self) -> &ArchivedHtmlEntry {
        let s = &self.archive.mmap[self.offset..self.offset + self.len];
        // SAFETY: `new` bytecheck-validated this exact slice, and the mapping is immutable.
        unsafe { rkyv::access_unchecked::<ArchivedHtmlEntry>(s) }
    }

    fn frames(&self) -> &[u8] {
        self.archive
            .bundle_strings(self.bundle)
            .expect("validated at construction; the mapping is immutable")
            .frames()
    }

    /// The per-call [`LazyState`] over this document's blocks — [`Doc::lazy_state`], but the
    /// bundle's frame blob is re-resolved through the `Arc` (the handle cannot hold a
    /// self-referential borrow of its own archive).
    fn lazy_state(&self) -> LazyState<'_> {
        LazyState {
            bufs: &self.bufs,
            frames: self.frames(),
            frame_starts: &self.blocks.frame_starts,
            raw_starts: &self.blocks.raw_starts,
            base: self.blocks.base(),
            len: self.blocks.raw_len(),
            decoder: &self.archive.decoder,
        }
    }

    /// Run `f` with a query view over this document — [`Doc::with_dom`], but the [`LazyState`]
    /// borrows the handle's own inflate caches, so each block decompresses at most once per
    /// handle.
    fn with_dom<R>(&self, f: impl FnOnce(&ArchivedDom<'_>) -> R) -> R {
        let state = self.lazy_state();
        let dom = self.entry().bind(StringSource::lazy(&state));
        f(&dom)
    }
}

impl Debug for OwnedDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedDoc")
            .field("pos", &self.pos)
            .finish_non_exhaustive()
    }
}

impl DomRead for OwnedDoc {
    const IS_IMMUTABLE: bool = true;

    // Same contiguous-DFS blob as `Doc`, so the same layout-exploiting linear sweep.
    type Forward<'a> = LinearSweep<'a, Self>;
    type Descendants<'a> = LinearSweep<'a, Self>;

    fn forward_from(&self, start: NodeIndex) -> Self::Forward<'_> {
        LinearSweep::forwards_at(self, start)
    }

    fn descendants_from(&self, start: NodeIndex) -> Self::Descendants<'_> {
        LinearSweep::descendants_at(self, start)
    }

    fn with_view<F: FnOnce(DomView<'_>) -> R, R>(&self, f: F) -> R {
        self.with_dom(|dom| dom.with_view(f))
    }

    fn with_nodes<F: FnOnce(NodesView<'_>) -> R, R>(&self, f: F) -> R {
        self.with_dom(|dom| dom.with_nodes(f))
    }

    fn walk_view(&self) -> Option<DomView<'_>> {
        // Text-less bound view for resolved selector matching — see `Doc::walk_view`.
        Some(self.entry().bind(StringSource::plain(&[])).view())
    }

    fn root(&self) -> HtmlElement<'_, Self> {
        HtmlElement::root(self)
    }

    fn repackage(&self) -> DomInner {
        self.with_dom(|dom| dom.repackage())
    }
}

impl DomRef for OwnedDoc {
    /// See [`Doc`]'s `dom_view`: materialise the whole pool once into the handle's `full` cache,
    /// then read it zero-copy via `Plain`.
    fn dom_view(&self) -> DomView<'_> {
        let text = self
            .full
            .get_or_init(|| StringSource::lazy(&self.lazy_state()).materialize());
        self.entry().bind(StringSource::plain(text)).view()
    }
}

impl ContiguousDfs for OwnedDoc {}
