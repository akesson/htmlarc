# 0001 — Two-lane, per-bundle string storage

- **Status:** Accepted — partially implemented. **Lane B compression shipped** (relocation in
  [0006](0006-per-bundle-string-relocation.md) format v9; compression in
  [0005](0005-per-document-string-compression.md) format v10). **Lane A index-comparison search +
  per-bundle shared dictionary deferred** ([0002](0002-unified-symbol-stores-extended-names.md)
  PR 7 — only ~4.3% of the compressed general-web archive; the query path resolves doc-local
  anyway). Lane A storage (per-doc dedup via the unified `SymbolTable`) did ship in 0002.
- **Date:** 2026-06-10
- **Scope:** `htmlarc-archive` (on-disk format) and `htmlarc-dom` (`DomInner` stores, query layer)
- **Supersedes:** the per-bundle shared-string-pool sketch reserved by the v4 `BundleDesc { data_offset, data_len }` slot

## Context

Today every document owns its string storage. `DomInner` holds `attrs`, `dataattrs`,
`classes` (each an `AttributeStore`/`ClassStore` with an embedded `StringHeap`) plus a flat
`StringStack` for text/comment payload. Strings are therefore deduplicated **within a
document** (the pure-topology win) but duplicated **across documents** — every page that
uses `mw-parser-output` stores that string again.

The next size lever is to stop duplicating strings across documents. The naive framing —
"hoist all strings into a per-bundle shared pool behind one indirection" — turned out to be
wrong once measured. This record captures the design we landed on instead, and the
measurements that forced it.

The product target is **TB-scale corpora spanning many regions and languages**, not just a
single Wiktionary dump. That constraint is load-bearing for several choices below.

All figures were measured with throwaway probes against
`cli/zim2htmlarc/testdata/wiktionary_en_all_nopic_2026-05.zim` (8.87 M HTML docs, 8.5 GB);
the probes were not kept.

## What the measurements showed

1. **Import is parse-bound, and cheap enough to be serial.** CPU split (release,
   single-thread): ZIM cluster decompression **~5–9 %**, HTML parse **~82 %**, entry build
   (optimal-width + checksum) **~13 %**. zstd decompresses at ~3–5 GiB/s. A *fully serial*
   import of the whole corpus is ≈ 6 min (parse + build). → Import does not need
   parallelism, and decompression is never worth parallelizing on its own.

2. **Class tokens are sharply bimodal.** 429,337 distinct tokens in 203 k docs; **98.4 % are
   singletons** that appear in exactly one document — they are MediaWiki `page-<title>` /
   `rootpage-<title>` classes (the page title embedded as a class). The **542** genuinely
   common classes (in 501+ docs) carry **94.8 % of all occurrences** in ~9 KiB. The
   vocabulary does not saturate — it grows ~linearly and crosses the `u16` ceiling (65,536)
   by ~28 k docs.

3. **Attributes split by occurrence vs. bytes.** By *occurrence* they are dominated by
   low-cardinality, exact-searched names (`lang`: 351 k refs → 1,210 distinct values, 6 KiB;
   `rel`/`type`: 2–4 distinct). By *bytes* they are dominated by high-cardinality content
   (`href`: 4.1 MiB, 93 % unique; `title`: 830 KiB, 88 % unique; `data-ety-tree-json`:
   1.3 MiB of unique embedded JSON from 419 elements). Cardinality is **corpus-dependent**:
   `lang` is ~1,210 distinct here (multilingual) but would be ~3 on a monolingual wiki.

4. **Compression strongly prefers aggregation.** zstd-19 on tag-stripped text:
   per-doc **independent** = 5.6×, per-**bundle** single frame = 24.4×, per-doc with a
   **shared trained dictionary** = 16.5×. Independent per-doc compression is **4.4× larger**
   than a per-bundle frame (≈ 5.8 GB vs ≈ 1.3 GB extrapolated to the corpus). A shared
   dictionary recovers most of the gap (~1.5× of per-bundle) while keeping per-document
   random access.

## Decision

Store strings in **two lanes with opposite treatment**, scoped **per bundle**.

### Lane A — deduplicated, raw, indexed

For strings that are **exact-searched or low-cardinality**: class tokens, `id`, and
structural attributes (`lang`, `rel`, `type`, `role`, `name`, table attributes…).

- Stored **raw (uncompressed)** so they stay memory-mappable and comparable by index.
- Referenced by a **namespaced `u16` index**: a reserved low range (~1000 slots) is the
  **per-bundle shared dictionary** of common values; the high range is **per-document
  local**. Resolution is a branchless range check (`idx < K ? shared[idx] : local[idx-K]`)
  — no discriminator bit, no reference widening.
- The reserved boundary is a **fixed round number**, not `shared.len()`, so adding a shared
  entry never renumbers local indices (stability across re-saves and diffs).
- Per-document local space is ample: a document has only ~30–60 distinct classes, far under
  the ~64.5 k local slots, so it never overflows.

This unlocks the **index-comparison search** optimization: resolve a selector string to its
index once per bundle, then integer-compare per node, and **skip an entire bundle** when the
index is absent. (Currently unrealized — `DomView::has_classes` resolves each node's id back
to a `&str` and string-compares; dedup is exploited for storage only.)

### Lane B — compressed, per-bundle zstd

For **high-cardinality content**: between-element text/comment payload and content
attributes (`href`, `title`, `data-*` blobs, and `src`/`alt`/`srcset` on media corpora).

- Compressed with zstd using a **per-bundle** dictionary/frame, trained on that bundle's own
  (linguistically coherent) content.
- Framing choice is left open (see Open questions): a single per-bundle frame gives the best
  ratio but requires decompressing the whole bundle to read one document; per-document blobs
  against a per-bundle-trained dictionary cost ~1.5× more but keep single-document random
  access.
- Compressing Lane B means **giving up zero-copy mmap** for that data (decompress on read).
  This is a deliberate trade: Lane B is the byte bulk, decompression is ~GiB/s, and the
  `probe` sweep already reads bundle-sequentially.

### Routing rule

Cardinality is corpus-dependent, so do **not** hardcode a per-name allowlist:

1. **Exact-searched names always go to Lane A** (`id`, class tokens, `lang`, `rel`, `type`,
   `role`, `name`, …) regardless of cardinality — searchability mandates the index, and they
   are cheap.
2. **Everything else is routed per bundle by observed cardinality** — low distinct-count →
   Lane A (cheap dedup); high distinct-count with real bytes → Lane B (compress).

### Scope: per-bundle, not corpus-wide

Both the Lane A shared dictionary and the Lane B zstd dictionary are **per-bundle**. For a
homogeneous single-language dump a corpus-wide dictionary would store the common set once,
but for the heterogeneous TB-scale target that is wrong: regional/language common sets
differ, and one zstd dictionary spread across many languages compresses far worse than a
per-bundle dictionary trained on coherent content. The "store once" saving is negligible
(~tens of MB on a TB) against the locality loss. Per-bundle scope also keeps bundles
**independent**, which is what makes parallel build and parallel re-save clean.

A bundle is a run of consecutive ZIM clusters (~10 k docs), owned end-to-end by one worker —
so the per-bundle dictionaries are built in-process during that bundle's parse, with no
shared mutable state across workers.

## Consequences

**Positive**

- Removes cross-document duplication of the bytes that matter, in the way each kind of string
  actually wants: dedup for the searched/low-card lane, compression for the content lane.
- Lane A enables index-comparison + bundle-skip search — a query-speed win on the hot
  `probe` path, not just a size win.
- Per-bundle independence preserves the existing parallel-build model and makes re-save
  (re-pack a bundle, or copy untouched bundles verbatim) parallel along bundle boundaries.
- Adaptive routing and per-bundle dictionaries survive heterogeneous, multi-language input.

**Negative / costs**

- Lane B is no longer zero-copy; reads decompress.
- More moving parts in the footer (per-bundle Lane A namespace boundary + dictionary, per-
  bundle Lane B dictionary) and a two-range resolution in the query layer.
- Replicating the truly-universal common set per bundle is mild waste (~tens of MB on a TB) —
  accepted in exchange for locality.

## Alternatives considered and rejected

- **General per-bundle string pool behind one indirection.** Rejected: with 98.4 % singletons
  a unified pool needs `u32` ids (or per-doc forwarding maps), costing several GB of
  reference/forwarding overhead to deduplicate strings that don't repeat — net-negative. The
  win is concentrated in ~542 strings, captured by the Lane A shared range instead.
- **Corpus-wide shared/frozen dictionary.** Rejected for the TB/multi-region target (see
  Scope). Correct only for a homogeneous corpus.
- **Pure bundle (all stores sealed at one boundary).** Rejected: the first store to overflow
  forces premature seals of the others (runts); replaced by per-store/per-doc independence.
- **Pure segmented (independent global per-store segments).** Rejected: serial import with no
  bundle independence, and — because the class vocabulary exceeds `u16` anyway — it gains no
  packing advantage over per-bundle here.
- **Compress per-document independently.** Rejected: 4.4× larger than aggregated (measurement
  4).

## Open questions (tuning, not blocking)

- ~~Lane B framing: per-bundle frame vs. per-document-with-per-bundle-dictionary.~~ **Resolved by
  [0005]:** per-**document** frames against one **archive-wide** dictionary — *diverging* from this
  ADR's per-bundle leaning. Per-document framing keeps single-document random access ≈ sequential
  (a per-bundle frame must inflate the whole bundle to read one document); a single global
  dictionary trains once instead of per bundle and is a safe floor on multilingual data. See [0005]
  for the measurements that forced the divergence.
- Reserved Lane A range size (1000 is a reasonable start for classes; per-name spaces).
- The cardinality threshold for routing non-searched attributes to Lane B.
- Attribute substring operators (`[href^=…]`, `*=`, `$=`) cannot use the index; they
  decompress and scan Lane B.

## Extraction opportunities surfaced

Two large byte sinks are *derivable* rather than worth storing, consistent with the
"extraction is the product" direction:

- `page-<title>` / `rootpage-<title>` classes (~98 % of the class vocabulary) are derivable
  from the document key — drop and reconstruct on render.
- `data-ety-tree-json` (1.3 MiB of unique embedded JSON from 419 elements) is machine data
  smuggled into an attribute — a candidate to externalize or drop rather than compress.

## Implementation notes

- The v4 `BundleDesc { data_offset, data_len }` reserved slot was the hook for a per-bundle
  shared store; it becomes the per-bundle Lane A dictionary + Lane B dictionary region. The
  v4 doc table / sort index / bundle table are unaffected.
- The query layer (`DomView::has_classes` / `has_id` / `has_attributes`) must change from
  resolve-id→string + string-compare to resolve-query→index-once + integer-compare, with the
  per-document local fallback for high-range indices.
- The existing per-document rebuild (`DomInner::rebuild`, the `*ReBuilder`s) already performs
  the mark-used + reindex pass that re-pack/compaction needs; re-pack is import minus parse.
