# 0006 — Per-bundle string relocation (format v9)

- **Status:** Accepted — implemented (PR #29, format v9). The relocated block is **compressed**
  by [0005](0005-per-document-string-compression.md) (v10); this record documents the
  uncompressed relocation v9 introduced.
- **Date:** 2026-06-18 *(recorded retroactively 2026-06-19 — the work shipped before its ADR, so
  the out-of-order number is intentional; it sits chronologically between [0004] and [0005].)*
- **Scope:** `crates/htmlarc-archive` (format v9, `bundle_strings`, writer/reader, mmap),
  `crates/htmlarc-dom` (`StringSource` read seam)
- **Companion:** the structural half of [0001](0001-string-storage-lanes.md)'s Lane B — relocates
  the text/comment payload into a per-bundle block so [0005] can compress it; sized against the
  bundle of [0004](0004-bundle-size-1000.md).

## Context

Through format v8 every document stored its own text/comment payload (`StringStack`) inside its
per-document rkyv blob. ADR [0001] established Lane B — the high-cardinality content lane — but
its first step is purely structural: get every document's text out of the per-document blob and
into **one contiguous per-bundle region**, so that a later codec can compress a whole bundle's
worth of text at once (the per-bundle region is the compression window of [0004]).

Two prerequisites had to land first so the relocated block could be read efficiently and
codec-agnostically:

- **PR #28** made every document sweep **bundle-contiguous** (`HtmlArchive::bundle_count()` /
  `bundle_range()` mirroring `MmapArchive`), so a per-bundle block can be touched once per bundle
  rather than re-resolved per document.
- **PR #26** made `probe` output **fully owned** (dropped the `'dom` borrow), so a document can be
  read through a transient per-bundle buffer rather than a borrow into the mmap.

## Decision

**Format v9** (PR #29): relocate each document's text/comment pool out of its per-document rkyv
blob into a single per-bundle block.

- **Block** (`bundle_strings.rs`): a new `BundleStrings` stores every document's relocated payload
  in the bundle's reserved data region, with a per-document offset table (prefix sum). Node
  `text_range`s stay document-local and index into each document's segment of the block. **Stored
  uncompressed** in v9 — a read is a zero-copy mmap slice.
- **Read seam** (`htmlarc-dom`, `string_source.rs`): a new `StringSource<'a>` is the single point
  `DomView` reads text through. `Plain(&[u8])` is verbatim bytes (the only arm used in v9). A
  second arm, `Lazy(&LazyState)`, was added as a **dormant compression seam** — defined and tested
  but unused — so the dom crate could stay codec-agnostic while the archive layer later injects a
  decoder. The seam keeps `htmlarc-dom` free of any compression dependency.

No back-compat shim — v8 and older must be re-packed (pre-1.0 clean-slate stance).

## Consequences

- The per-bundle data region that [0004] described as "reserved but empty" is now **populated**
  (the relocated text block); the bundle table gains the block's offset/length.
- The node blob (topology) is **byte-identical** — relocation moves only the text payload, so the
  topology size lever stays clean.
- **v9 left the block uncompressed.** [0005] (format v10) fills the dormant `Lazy` seam: it
  compresses each document's segment as an independent zstd frame against one archive-wide
  dictionary, turning the zero-copy `Plain` read into a lazy per-document inflate. This ADR's v9
  layout is therefore **superseded in part** by [0005] — the relocation stands; its bytes are now
  a compressed encoding.
