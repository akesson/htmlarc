# 0002 — Unified symbol stores, extended names, and adaptive ref widths

- **Status:** Accepted (pending implementation)
- **Date:** 2026-06-10
- **Scope:** `htmlarc-dom` (stores, node format, parser, formatter, query layer),
  `htmlarc-archive` (bundle footer), `cli/htmlarc-convert`
- **Companion:** [0001](0001-string-storage-lanes.md) (two-lane storage). Amends one point:
  the Lane A shared range moves from the **low** end of the `u16` space to the **top** end.

## Context

Two intertwined changes are pending:

1. **Extended names.** `HtmlTag` (134/256 used) and `HtmlAttr` (188/256 used) are closed
   enums; unknown tags and non-`data-` attributes are hard errors that fail the whole
   document. SVG, MathML, and custom elements are unrepresentable; `<svg>`/`<math>`
   subtrees are dropped wholesale.
2. **ADR 0001's per-bundle string storage** (Lane A dedup dictionary, Lane B zstd).

Both are parameterizations of the same store machinery. Today three near-identical store
families exist — `AttributeStore` `(u8,u16)`, `DataAttributeStore` `(u16,u16)`,
`ClassStore` `u16` — each with its own heap, builder, rebuilder, view, and list module.
Their sorted-position ids force `shift_values_from` (an `O(n)` rewrite of every list
entry) on live insert, and force builders to be parallel types reindexed at `build()`.

Latent ceilings, relevant now that the target corpus is **general scraped HTML** (not
just wiki dumps): `StringHeap::insert` silently wraps past 65,535 strings
(`stringheap.rs`); `ListVec` silently wraps past its list-entry ceiling
(`listvec/mod.rs`) — and that ceiling is **32,768, not 65,535**: `ListInfo`'s next-pointer
is 15 bits (bit 15 is the head flag), so list *tails* are addressable only to `0x7FFF`
(heads, reached directly via `ListIndex`, keep the `0xFFFE` node-slot ceiling). All
reachable today, since nodes were lifted to u24 (16.7M) while a list entry exists per
(node × class/attr occurrence). Pathological-but-real inputs: single-page specs with tens
of thousands of `id`s, utility-CSS sites with thousands of class tokens.

PR 1 (shipped) converts these silent wraps into checked per-document parse errors (the
`try_insert`/`try_new_list`/`try_append` APIs + builder poison flags), so they bound
correctness rather than corrupt it.

## Decision

### 1. One per-document `SymbolTable`, stable ids, permutation indexes

A single per-doc table holds every deduplicated identity string: class tokens, extended
tag names, extended attribute names, Lane A attribute values. Structure: the existing
`StringHeap` (insertion-ordered bytes) + a content-sorted permutation `Vec<u16>` for
`find(&str) -> Option<Sym>`.

All ids (`Sym`, entry ids) are **insertion-ordered and stable**; sortedness lives in
separate permutation vecs. Consequences: live insert is append + one permutation memmove
(`shift_values_from` is deleted); a parse builder is the store plus a transient
`StringInterner`/`HashTable`, and `build()` = sort permutations + drop hash tables (the
parallel `*StoreBuilder` types and their `reindex_value` passes are deleted); entry tables
sort and binary-search **numerically**, which also works for Lane B refs without
decompression.

### 2. Namespaced `u16` reference spaces, shared range at the top

Every reference space splits into fixed ranges, scope-ordered: static (enum) low,
document-local middle, bundle-shared top. Resolution is one compare + offset.
`0xFFFF` stays the universal "none" sentinel.

| space | layout (provisional constants, frozen at PR 7) |
|---|---|
| tag byte | `[0,192)` enum (134 used, 57 promotion headroom) · `[192,255)` per-doc vocab (63 slots) · `255` overflow → `NodeIndex`-keyed side map |
| `NameSym` (u16) | `[0,256)` `HtmlAttr` repr · `[256,0xF000)` local sym + 256 · `[0xF000,0xFFFF)` shared · `0xFFFF` none |
| `ValueRef` / class `Sym` (u16) | `[0,0xF000)` local · `[0xF000,0xFFFF)` shared · `0xFFFF` none |

Unified `LOCAL_CAP = 0xEF00` (61,184 — addressable from every space, including the
+256-biased `NameSym`); shared capacity 4,095 (sized for utility-CSS class vocabularies,
larger than 0001's ~1000; unused slots cost nothing — only the table's length serializes).
Boundaries are compile-time constants so a document's local refs are identical whether or
not it is in a bundle; the shared table's *content* is per-bundle (footer).

*Amendment to 0001:* shared range at the top, not the bottom — the local path (98%+ of
refs) resolves with zero bias, the layout composes with the enum low range as a scope
gradient, and it generalizes if ref widths grow.

### 3. One attribute store; classes become bare lists; the node loses a slot

Standard, `data-*`, and unknown attributes merge into one `AttrStore`:
`ListVec` + `entries: Vec<(NameSym, ValueRef)>` + numeric permutation. `data-*` names are
ordinary extended names (full name stored; the `data-` prefix-stripping special case and
its edge cases are deleted). Unknown attribute names stop being parse errors (keep the
leading-alphanumeric junk filter). Public API: `Attribute { name: AttrName<'a>, val }`
with `enum AttrName { Std(HtmlAttr), Ext(&'a str) }`; `DataAttribute` is deleted.
Attribute source order is restored (today std and data attrs live in two lists and
interleaving is lost).

`ClassStore` reduces to a `ListVec` whose values are `Sym`s directly — its sorted table,
builder, and rebuilder are deleted; the symbol table's content permutation does the
resolve.

The node record drops the data-attr slot: `node_size` 22→20 (U24), 17→15 (U16) —
−9–12% on the topology blob of every document. The text-node overlay still fits
(`1+3·slot+8 ≤ node_size`; exactly 15=15 at U16 — keep an explicit test).

Query layer realizes 0001's index-comparison search: resolve a selector string to a
`Sym`/entry id once per doc (later: once per bundle, with bundle-skip on miss), then
integer-compare per node — replacing the per-node string compares in
`has_classes`/`has_id`/`has_attributes`.

### 4. Extended tags via a per-document vocab inside the tag byte

The node keeps its 1-byte tag. Bytes `≥ EXT_BASE` index `ext_tags: Vec<Sym>` (per-doc,
≤63 entries); `255` is an overflow sentinel falling back to a rare side map.
`nodes.tag()` stays infallible by normalizing any vocab byte to `HtmlTag::extended`, so
every `matches!` classifier compiles and defaults sanely (extended = non-void,
non-raw-text, never auto-close); `tag_name()` resolves the real name. Unknown tags stop
being parse errors. `hnan`/`figure_inline` can be demoted out of the enum (they exist
only because it was closed); MathML/SVG promotions into the enum become a data-driven
follow-up, budgeted by `EXT_BASE`.

### 5. Foreign content stored; case fixed at render

The tokenizer's `<svg>`/`<math>` skip machinery is deleted; foreign subtrees parse as
normal elements using extended tags/attrs. html5gum lowercases names before we see them;
the WHATWG foreign-content adjustment tables (`clippath`→`clipPath`, `viewbox`→`viewBox`,
…) restore case **at the formatter** for all known SVG/MathML names — no storage change.
Custom-element names are lowercase by spec. Childless extended elements render as
`<name></name>` (valid in HTML and SVG-in-HTML; no per-node self-closing flag).

### 6. Mutable ⇒ wide; narrow widths are an archive-only concept

Width is pure byte-packing — ids never change across widths. Lifecycle rule, uniformly
for nodes and refs: parse/rebuild builds wide (already true for nodes); `repackage()`
inflates to wide as part of the copy it already makes; serialize down-packs each store
independently (`into_optimal_width` generalized); archived reads stay zero-copy at stored
width via width-dispatched views. This deletes the mid-edit overflow class structurally,
along with `DOWNPACK_MARGIN` (save U16 iff it fits). v1 ships u16 refs only, with
**checked** inserts (per-document errors, replacing today's silent wraps) and a width
flag reserved in the store headers; u24 refs are implemented only if the probe demands.

### 7. Bundle artifacts (per 0001)

The bundle footer holds three siblings, unified only in scope and lifecycle: the Lane A
shared `SymbolTable` (raw, mmap'd, index-addressed; serves class tokens, searched values,
**and extended names** — `mrow`, `path`, `data-mw` stored once per bundle), the Lane B
zstd dictionary (opaque compressor state), and the per-name routing table. Docs parse
fully local; the bundle worker freezes the shared dict from cross-doc frequency and
rewrites refs local→shared — a per-doc reindex (existing rebuilder pattern),
embarrassingly parallel. The Lane B routing rule is the sym-space pressure valve:
unbounded-cardinality values (`style`, `href`, `srcset`, SVG `d`, `data-*` JSON) never
enter the symbol space; `id` is the one unbounded Lane A resident.

## Implementation plan

Each PR lands independently with tests green. Constants freeze at PR 7; before that they
are internal and adjustable. Benchmarks tracked at every step: parse throughput, selector/
iteration benches, `.htmlarc` size on the wiktionary fixtures.

1. **Guardrails + probe** (no format change). Checked inserts in `StringHeap`/`ListVec`
   (silent wrap → per-doc error). Probe tool over wiktionary + a general scraped-HTML
   sample measuring per-doc: distinct syms (classes ∪ ids ∪ searched values ∪ non-enum
   names), total list entries, heap strings; per-bundle shared-dict hit rate at 1k/4k
   slots. *Gate output:* `LOCAL_CAP`/`SHARED` capacity/`EXT_BASE` confirmation; whether
   PR 6 must implement u24 refs.
2. **`SymbolTable` + classes port.** New `SymbolTable` (heap + permutation + interner);
   `ClassStore` family deleted, class lists hold `Sym`s; `has_classes` goes
   resolve-once + integer-compare. Format bump. *Gates:* selector bench (expect win),
   parse/size neutral.
3. **Unified `AttrStore`.** `(NameSym, ValueRef)` entries; tokenizer accepts unknown
   attr names; `DataAttributeStore` family deleted; node data slot dropped (node_size
   20/15, U16 text-overlay boundary test); public `Attribute`/`AttrName` API;
   width-flag byte reserved in store headers. Format bump. *Gates:* parse within ~2%,
   topology −9–12%, attr-order round-trip snapshots. This is the big-bang PR — parser,
   stores, fmt, query, rebuilder — no smaller honest slice exists.
4. **Extended tags.** `ext_tags` vocab + byte ranges + overflow side map +
   `HtmlTag::extended` normalization + `tag_name()`; tokenizer accepts unknown tags;
   formatter renders extended names; demote `hnan`/`figure_inline`. Format bump.
   *Gates:* custom-element round-trip fixtures, parse neutral.
5. **Foreign content.** Delete skip machinery; svg/math subtrees stored; WHATWG case
   adjustment tables at the formatter; childless-extended render rule; CDATA-in-foreign
   tests. *Gates:* svg/math round-trip fixtures; **size growth measured and documented**
   (storing previously-dropped content is the intended product change).
6. **Mutable ⇒ wide.** `repackage()` widens; `DOWNPACK_MARGIN` removed;
   `into_optimal_width` generalized per store; u24 refs implemented only if PR 1 says so.
   *Gates:* owned/archived byte-identity spike tests, edit-after-load tests.
7. **Bundle Lane A** (`htmlarc-archive`). Footer dict; freeze + parallel per-doc reindex;
   `DomView` two-table resolution; bundle-skip in the selector engine; constants frozen.
   Archive format bump. *Gates:* corpus size reduction, bundle-skip query bench, import
   time vs ~6 min serial baseline.
8. **Lane B.** Per-name routing table (searched → A; cardinality threshold → B); zstd
   lane with framing decision (0001's open question, decided by probe-sweep vs serving
   workload); decompress-and-scan path for substring selectors. *Gates:* corpus size
   (0001 measured 4.4× headroom on text), probe-sweep throughput.

Follow-ups, not in scope: enum promotion curation (MathML core / common SVG, data-driven
after PR 5); deriving `page-<title>` classes and externalizing `data-ety-tree-json`
(0001 §Extraction opportunities).

## Measured — PR 1 gate (2026-06-11)

`htmlarc-convert stats` over **`wiktionary_en_all_nopic_2026-05.zim`** — 8,868,024 docs,
879 bundles, 67 s wall on 14 cores, 5.7 GB peak RSS. Per-document distribution (log-bucket
percentiles, exact max) and the per-bundle Lane A shared-dictionary simulation:

| metric | p50 | p99 | p99.9 | max | cap | over cap |
|---|---|---|---|---|---|---|
| nodes | 255 | 4,095 | 8,191 | 55,373 | 16,777,215 | 0 |
| max_depth | 15 | 31 | 31 | 50 | 256 | 0 |
| list_entries | 255 | 2,047 | 8,191 | 44,232 | 32,768 | **10** |
| distinct_pairs | 63 | 511 | 1,023 | 10,299 | 65,535 | 0 |
| ext_tag_names | 0 | 0 | 0 | 17 | 63 | 0 |
| ext_attr_names | 1 | 15 | 15 | 23 | — | — |
| sym_union | 127 | 255 | 511 | 3,927 | 61,184 | 0 |

Shared dict: **K=1024 → 95.6 % mean** Lane A reference coverage (95.0 % worst bundle);
**K=4095 → 97.5 % mean** (97.0 % worst); ~12.2 GiB saved corpus-wide. (Smaller
`wiktionary_co.zim`, 9,567 docs: max sym_union 217, K=1024 = 97.8 %.)

Conclusions, and how they move the open decisions:

- **`LOCAL_CAP` (61,184) is hugely comfortable here** — worst `sym_union` is 3,927, ~16× under.
  No u24-ref pressure *from wiki*. But wiki per-doc vocabularies are tiny; the general-web run
  below tells a very different story.
- **The 32,768 list-entry ceiling is real and crossed**: 10 / 8.87 M docs exceed it (worst the
  `-i` page at 44,232 — huge inflection tables). Pre-PR-1 these silently corrupted their lists;
  now they are cleanly skipped (a 0.0001 % loss on wiki) until the redesign lifts the ceiling.
  Confirms the guardrail is load-bearing and that lifting this ceiling is a real goal, not
  hypothetical.
- **`EXT_BASE` 63-slot vocab and depth 256 are safe** for wiki (max 17 ext tags, depth 50). The
  64→256 depth bump is validated headroom; general web will sit higher.
- **1024 vs 4095 shared slots**: on homogeneous wiki, 4095 buys only +1.9 % coverage over 1024.
  The general-web case below tells a very different story.

### General web — Common Crawl

`stats` over **4 full CC-MAIN-2024-10 WARC segments** (spread across the crawl) — 136,447
HTML response docs, 14 bundles, 91 s, ~30 GB RSS (the in-memory WARC read held all four):

| metric | p50 | p99 | p99.9 | max | cap | over cap |
|---|---|---|---|---|---|---|
| nodes | 2,047 | 16,383 | 32,767 | 84,356 | 16,777,215 | 0 |
| max_depth | 31 | 127 | 1,023 | **9,047** | 256 | **487** (0.36 %) |
| list_entries | 2,047 | 16,383 | 32,767 | **71,682** | 32,768 | **30** (0.022 %) |
| distinct_pairs | 511 | 4,095 | 8,191 | 32,436 | 65,535 | 0 |
| ext_tag_names | 1 | 31 | 127 | **2,658** | 63 | **226** (0.17 %) |
| ext_attr_names | 31 | 1,023 | 2,047 | **32,039** | — | — |
| sym_union | 511 | 2,047 | 4,095 | **32,229** | 61,184 | 0 |

Shared dict, stable across the 14 bundles: **K=1024 → 22.0 % mean** (20.7–23.7 %),
**K=4095 → 34.9 % mean** (33.9–36.2 %); 160 MiB vs 237 MiB saved. (An earlier 8,417-doc
200 MB prefix agreed: 23.9 % / 36.4 %.)

This leg is the one that matters, and it **moves four things**:

- **`LOCAL_CAP` is the real question now.** The worst `sym_union` is **32,229 — 53 % of
  `LOCAL_CAP` (61,184)** on only 136 k docs, driven by `ext_attr_names` (max 32,039: a
  generated/framework-attribute page). The wiki margin of ~16× has collapsed to <2×, and the
  max climbs with sample size. → **PR 6 u24 refs are likely required for general web** (or the
  rare per-million doc that exceeds 61,184 syms is skipped). The decision is no longer "open
  pending data" — the data leans **toward implementing u24**; confirm against a multi-million
  run before freezing. The driver is *extended attribute names*, not classes/ids, so Lane B
  routing cannot relieve it (names are always Lane A).
- **Depth 256 too low; the tail is long.** 0.36 % of docs exceed 256, max **9,047**. Even a
  heap `Vec` sanity cap should be generous (≈ 8 k still clips the worst); kept at 256 for PR 1
  (skipped, not crashed) per the decision above.
- **`EXT_BASE` 63-slot vocab too small.** 0.17 % of docs exceed 63 distinct extended tags,
  max **2,658** (auto-generated / web-component pages). The overflow side-map stays rare
  (0.17 %) but can be large when it fires — design it for thousands of entries, not a handful.
- **Shared dict: 4095 ≫ 1024, and absolutely low.** 34.9 % vs 22.0 % mean (+12.9 pp, vs
  wiki's +1.9 pp) — confirms 4095 over 0001's ~1000 and **per-bundle, not corpus-wide**. But
  even 4095 covers only ~35 % of general-web Lane A refs (vs 97 % on wiki); a larger shared
  range and Lane B compression carry more of the general-web weight than the Lane A dict does.

The 32,768 list ceiling is crossed on both corpora (wiki and general web).

## Open questions

- Reserved low `ValueRef` specials (e.g. interned-empty for boolean attrs) — decide in PR 3.
- Exact `EXT_BASE` vs. enum-promotion budget — after PR 1 probe + PR 5 corpus data.
- Lane B framing (per-bundle frame vs per-doc blobs + trained dict) — PR 8, per 0001.
