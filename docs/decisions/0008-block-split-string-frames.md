# 0008 — Block-split string frames cut at text-node boundaries

- **Status:** Accepted
- **Date:** 2026-07-10
- **Scope:** `crates/htmlarc-archive` (format v11, `bundle_strings`, `codec`, writer/reader,
  `mmap`), `crates/htmlarc-dom` (`LazyState`/`StringSource::get`), `cli/htmlarc-convert`
  (per-block compression in both convert phases)
- **Companion:** refines the per-document zstd frames of
  [0005](0005-per-document-string-compression.md) (format v10) inside the per-bundle
  [`BundleStrings`](../../crates/htmlarc-archive/src/bundle_strings.rs) region of
  [0006](0006-per-bundle-string-relocation.md); keeps 0005's archive-wide dictionary and level
  unchanged.

## Context

v10 compressed each document's relocated text/comment pool as **one** zstd frame, and
`StringSource::get` inflated the **whole** frame on the first text read. That is the right shape
for whole-document reads, but a selective sweep pays far beyond what it touches: on a 5,000-doc
`cc_000` archive, a `h1, h2, h3` + `text_content()` sweep spent ~90–120 ms inflating ~196 MB of
raw pools to read ~1.5 MB of headings — the dominant term in the sweep, capping the visible
re-query margin over in-memory DOM libraries at ~1.1–2× while the select walk itself is 3–7×
faster. (Diagnosed 2026-07-10 by splitting sweeps into select-only / select+extract /
warm-extract passes; the per-frame *decoder-context* cost was separately measured and refuted as
a lever — the cost is the inflated bytes themselves.)

Measured block economics on the real cc pools (dictionary-trained, L3, first-block-only reads as
the sweep proxy):

- **16 KiB blocks:** +13.9% on the stored string block ≈ **+2% archive**; selective inflate ~3×
  cheaper (proxy: 120 ms whole-frame → 44 ms first-block).
- **8 KiB blocks:** +22.8% on the string block for only marginally less inflate (28 ms) — past
  the knee.
- Whole-pool reads (e.g. `doc.text`, repackage) get ~+20% slower — they now decode several
  frames instead of one.
- Wiktionary-class corpora (pool p50 ≈ 0.5 KB) are naturally single-block: layout and cost
  identical to v10.
- Storing text uncompressed instead (v9 layout) was rejected: cc archive 274 → 429 MB (1.56×,
  larger than the source HTML).

## Decision

Format **v11**: each document's pool is split into ~**16 KiB** blocks (`TEXT_BLOCK_SIZE`, a
target, not a maximum) **cut at text-node boundaries**, each block an independent zstd-L3 frame
against the same archive-wide dictionary. Reads inflate only the touched block.

- **Cut invariant** (`bundle_strings::block_cuts`): every `StringSource::get` is exactly one
  text node's contiguous range (`DomView::string_at`), and pool ranges are always mutually
  disjoint (the pool only appends — even a mutated document's superseded ranges are disjoint).
  Cutting only at range ends therefore guarantees no read ever straddles a block: `get` stays a
  borrowed `&str` slice, no stitching. Ranges are sorted before cutting (mutated documents can
  hold them out of document order); the final cut is forced to the pool length so dead tail
  bytes stay covered; a single node larger than the target becomes one oversized block; blocks
  never straddle documents.
- **Block** (`bundle_strings.rs`): `BundleStrings` becomes `frames` + three cumulative tables —
  `block_offsets` / `block_raw_offsets` (per block, bundle-cumulative) and `doc_blocks` (per
  document, mapping a slot to its block run). A text-free document owns zero blocks.
  `block_raw_offsets` staying bundle-cumulative `u32` keeps v10's 4 GiB-raw-text-per-bundle
  ceiling (ample at `BUNDLE_CAP` = 1,000 docs/bundle).
- **Read seam** (`string_source.rs`): `LazyState` carries per-doc *subslices* of the bundle's
  tables plus one `OnceLock` inflate cache per block; `get` picks the block by binary search
  (with a single-block fast path — the common case) and inflates it at most once. Empty ranges
  (`0..0` fresh nodes, `len..len` from `replace_text("")`, zero-block docs) return `""` before
  any lookup. Sweeps build the native tables + caches once per bundle
  (`ArchivedBundleStrings::arena`); single-doc handles (`Doc`/`OwnedDoc`) copy just their own
  `n_blocks + 1` entries. Whole-pool consumers (`materialize`, `dom_view`, in-memory
  rehydration) concatenate all blocks; `dom_view` keeps a separate flat-pool cache since
  `Plain` needs contiguous bytes.
- **Write path**: block boundaries are computed where the topology still owns its pool
  (`DomInner::text_ranges` before `take_string_pool`, in both `writer::push_entry` and
  `HtmlEntry::into_serialized`); compression goes through one shared helper
  (`SerializedEntry::compress_blocks` / `codec::compress_pool_blocks`) in both convert phases,
  and the writer refuses an entry whose pool was never compressed. Dictionary training still
  samples raw pools (0005 unchanged).

## Consequences

- Selective text sweeps over large documents inflate ~1/3 the bytes for +2% archive size; small-
  document corpora pay nothing.
- Whole-document text reads decode a few frames instead of one (~+20% on that path); the
  per-frame fixed cost is ~1.5 µs, negligible at ≤ a few blocks per document.
- A handle read through both `dom_view` (flat) and the lazy path holds its text twice; accepted
  — neither path pays for the other.
- v10 archives are no longer readable (exact-version check, pre-1.0 clean-slate policy):
  re-convert to upgrade.
