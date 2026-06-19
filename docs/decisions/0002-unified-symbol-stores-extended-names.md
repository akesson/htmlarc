# 0002 — Unified symbol stores, extended names, and adaptive ref widths

- **Status:** Accepted — implemented (PRs 1–5, formats v5–v8). **PRs 6 (mutable⇒wide / u24 refs)
  & 7 (per-bundle Lane A shared dictionary) deferred** — measured unnecessary at corpus scale
  (per-doc u16 widths never overflow except a handful of `RunVec` arenas; the shared dict saves
  only ~4.3% of the compressed general-web archive). See *Measured* below.
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

*(2026-06-11)* Shared capacity confirmed at the **4,096-slot scale** by the K-sweep, and
the range stays **reserved but dormant**: the dictionary itself is deferred (see §7) —
until it lands, every symbol is doc-local and shared-range refs simply never occur.

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

*Deferred (2026-06-15):* the per-document width probe (see §Measured — PR 6 deferral) showed
only the RunVec arena crosses u16 (6 docs / 2 M); `SymbolTable`/`AttrStore` sit at 74–78 % with
**0** crossings, and node>u16 docs (40) are already served by `NodeWidth::U24`. With PR 1's
checked inserts turning overflow into a clean per-doc skip, and **no downstream PR depending on
u24 refs**, PR 6 is deferred past the initial sequence — the width flag + per-store down-pack +
u24 path land at a later format bump, when a real document first crosses, exercised against real
overflow rather than a 0-doc code path. The reserved store width-flag stays unspent; the node
record stays at today's 15 B / 20 B.

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

*Deferred (2026-06-11):* the Lane A shared dictionary is **deferred past the initial PR
sequence**. The gate showed it is worth only ~4.3 % of the compressed general-web archive,
and the query path must resolve doc-local syms regardless, so the dict is an additive
optimization, not a structural dependency. Nothing regresses versus main, which has no
cross-doc string sharing today (the v5 "class-token sharing" win was within-doc dedup).
The top reference range stays reserved — it costs nothing and keeps resolution a single
compare when activated. Re-evaluate after Lane B and topology packing have landed, against
(a) wiki-shaped corpus size (on wiktionary_co Lane A, raw+dict at 272 KiB beats zstd at
1.2 MiB) and (b) a bundle-skip selector benchmark on the by-then integer-compare query
engine. The Lane B zstd dictionary and routing table (PR 8) are unaffected.

## Implementation plan

Each PR lands independently with tests green. Constants freeze at the last
format-touching PR of the sequence (PR 8, now that PR 7 is deferred); before that they
are internal and adjustable. Benchmarks tracked at every step: parse throughput, selector/
iteration benches, `.htmlarc` size on the wiktionary fixtures.

1. **Guardrails + probe** (no format change). Checked inserts in `StringHeap`/`ListVec`
   (silent wrap → per-doc error). Probe tool over wiktionary + a general scraped-HTML
   sample measuring per-doc: distinct syms (classes ∪ ids ∪ searched values ∪ non-enum
   names), total list entries, heap strings; per-bundle shared-dict hit rate at 1k/4k
   slots. *Gate output (done — see §Measured):* wiki comfortable; general web needs u24
   refs, a larger ext-tag side-map, and a higher depth cap; 4095 ≫ 1024 shared slots.
2. **`SymbolTable` + classes port** ✅ *(shipped — see §Measured PR 2 results).* New
   `SymbolTable` (heap + permutation + interner); `ClassStore` family deleted, class lists
   hold `Sym`s; class matching goes resolve-once + integer-compare. Format bump 4 → 5.
   *Gates met:* `select class` −6 %, parse −2.6 %, size neutral (−0.01 %). Contiguous-runs
   list storage split out to a follow-up (PR 2.5); `has_id`/attr value matching still string.

   **2.5 — Contiguous-run class lists** ✅ *(shipped — see §Measured PR 2.5 results).*
   `RunVec` arena replaces the class `ListVec` (2 B/entry + terminator vs 4 B/entry);
   node slots hold run starts directly; the linked list (and its u15 ceiling) survives
   only inside the attribute stores until PR 3 deletes them. Format bump 5 → 6.
   *Gates met:* size −0.93 %, class-select family ≈ −5 %, parse/repack/load neutral.
3. **Unified `AttrStore`** ✅ *(shipped — see §Measured PR 3 results).* `(NameSym, ValueRef)`
   entries; tokenizer accepts unknown attr names; `DataAttributeStore` + `ListVec` families
   deleted; node data slot dropped (node_size 22→20 / 17→15, U16 text-overlay boundary
   test); public `Attribute`/`AttrName` API; resolve-once id + attribute matching. Format
   bump 6 → 7. *Gates met:* size −4.1 % (wiktionary_co, the node-record shrink), repack
   −7.8 % / load −9.7 % (build-time reindex passes deleted), attr-order round-trip snapshots
   (pure reorder, no content change). The width-flag byte is **not** reserved here — that is
   PR 6's holistic mutable⇒wide work, not a speculative dead field now.
4. **Extended tags.** ✅ *(shipped — see §Measured PR 4 results).* `ext_tags` vocab + byte
   ranges + overflow side map + `HtmlTag::extended` normalization + `tag_name()`; tokenizer
   accepts unknown tags; formatter renders extended names; `hnan`/`figure_inline` demoted.
   `DomStack` reshaped to carry full tag identity; extended tag selectors resolve-once. Format
   bump 7 → 8. *Gates met:* size +0.44 % (the empty-`ext_tags` per-doc overhead), parse neutral
   (own comparison p = 0.49), custom-element round-trip + repackage-survival fixtures.
5. **Foreign content.** ✅ *(shipped — see §Measured PR 5 results).* Skip machinery deleted;
   svg/math subtrees stored as ordinary (extended) elements; WHATWG case adjustment tables at
   the formatter (`html/foreign.rs`); foreign-content depth in the tokenizer suppresses the
   raw-text switch and keeps `<![CDATA[…]]>` as character data; childless-extended render rule;
   extended tag selectors ASCII-case-insensitive. **Plus tolerant end-tag recovery** (an
   unplanned but load-bearing addition — see results): the strict tree builder now pops to a
   matching open ancestor instead of failing a document on an unclosed foreign child. No archive
   format bump (v8 layout unchanged). *Gates met:* svg/math/CDATA round-trip + recovery fixtures;
   size **byte-identical on wiktionary_co** (no foreign content) and **+26 % / +20 % coverage on a
   Common Crawl sample**; parse neutral.
6. **Mutable ⇒ wide** — **deferred (2026-06-15, see §6 and §Measured — PR 6 deferral).** The
   per-document width probe showed only the RunVec arena crosses u16 (6 docs / 2 M);
   `sym_union`/`distinct_pairs` sit at 74–78 % with **0** crossings, and node>u16 docs (40) are
   already served by `NodeWidth::U24`. PR 1's checked inserts make the 6 a clean skip, and no
   later PR depends on u24 refs, so the width flag + per-store down-pack + u24 refs land at a
   future format bump when a real doc crosses. *Original scope (when picked up):* `repackage()`
   widens; `DOWNPACK_MARGIN` removed; `into_optimal_width` generalized per store; u24 refs.
   *Gates:* owned/archived byte-identity spike tests, edit-after-load tests.
7. **Bundle Lane A** (`htmlarc-archive`) — **deferred (2026-06-11, see §7)**; re-evaluate
   after PR 8 + topology packing. When picked up: footer dict; freeze + parallel per-doc
   reindex; `DomView` two-table resolution; bundle-skip in the selector engine. Archive
   format bump. *Gates:* corpus size reduction, bundle-skip query bench, import time vs
   ~6 min serial baseline.
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

`stats` over **60 full CC-MAIN-2024-10 WARC segments** (evenly spread across the crawl's
90,000) — **2,041,140 HTML response docs**, 240 bundles, 17 min, 17.4 GB RSS (streamed one
segment at a time):

| metric | p50 | p99 | p99.9 | max | cap | over cap |
|---|---|---|---|---|---|---|
| nodes | 2,047 | 16,383 | 32,767 | 145,797 | 16,777,215 | 0 |
| max_depth | 31 | 127 | 1,023 | **54,017** | 256 | **7,791** (0.38 %) |
| list_entries | 2,047 | 16,383 | 32,767 | **84,452** | 32,768 | **353** (0.017 %) |
| distinct_pairs | 511 | 4,095 | 8,191 | 50,935 | 65,535 | 0 |
| ext_tag_names | 1 | 31 | 127 | **2,698** | 63 | **3,216** (0.16 %) |
| ext_attr_names | 31 | 1,023 | 2,047 | **46,854** | — | — |
| sym_union | 511 | 2,047 | 4,095 | **48,719** | 61,184 | 0 |

Shared dict, rock-stable across the 240 bundles: **K=1024 → 21.9 % mean** (19.6–23.7 %),
**K=4095 → 34.7 % mean** (32.3–36.6 %); 2.3 GiB vs 3.4 GiB saved.

This leg is the one that matters, and it **settles / sharpens four things**:

- **`LOCAL_CAP` → u24 refs are required for general web.** The worst `sym_union` grew
  **monotonically with sample size — 7,090 (8 k docs) → 32,229 (136 k) → 48,719 (2 M) = 79.6 %
  of `LOCAL_CAP` (61,184)**, with 0 crossings *so far*. The trend is unambiguous: it scales
  with corpus size, so at the TB / billion-doc target some docs **will** exceed 61,184. →
  **Implement u24 refs (PR 6)**, or accept skipping the rare doc (currently <1 in 2 M) that
  overflows the per-doc symbol space. The driver is **extended attribute names** (max 46,854 —
  generated/framework pages), which are always Lane A, so Lane B routing cannot relieve it.
  `distinct_pairs` (the attribute entries table) tracks the same curve (max 50,935 = 78 % of
  its u16 cap), reinforcing the u24 call.
- **Depth tail is extreme.** 0.38 % of docs exceed 256, max **54,017**. Kept at 256 for PR 1
  (skipped, not crashed); the redesign's heap `Vec` sanity cap must be very generous (≥ 64 k)
  or explicitly accept clipping the worst.
- **`EXT_BASE` 63-slot vocab too small.** 0.16 % of docs (≈ 1 in 600) exceed 63 distinct
  extended tags, max **2,698**. The overflow side-map must hold **thousands** of entries, not a
  handful — size it accordingly.
- **Shared dict: 4095 ≫ 1024, and absolutely low.** 34.7 % vs 21.9 % mean (+12.8 pp, vs wiki's
  +1.9 pp) — confirms 4095 over 0001's ~1000 and **per-bundle, not corpus-wide**. Even 4095
  covers only ~35 % of general-web Lane A refs (vs 97 % on wiki); **Lane B compression, not the
  Lane A shared dict, carries the general-web weight**.

The 32,768 list ceiling is crossed on both corpora. **Tooling finding:** html5gum
deep-recurses on some adversarial general-web HTML and overflowed the default 2 MiB worker
stack mid-tokenization (before any htmlarc depth guard, which sits in our tree builder);
worked around with 256 MiB worker stacks. A mark against html5gum for the
html5gum-vs-harden-own-parser decision — a hardened own tokenizer would bound this explicitly.

### K-sweep, index-width coverage, and lane economics (2026-06-11)

**Index-width coverage** (2.04 M general-web docs — what fraction does an N-bit field miss):

| metric | docs > 2¹⁵ (32,768) | docs > 2¹⁶ (65,536) | docs > 2¹⁷ |
|---|---|---|---|
| list_entries | 353 (0.0173 %) | **6 (0.0003 %)** | 0 |
| sym_union | 2 (0.0001 %) | 0 | 0 |
| distinct_pairs | 2 (0.0001 %) | 0 | 0 |

→ **u16 covers 99.9997 % of docs for list entries and 100 % observed for symbols/pairs.**
The current u15 (head-flag bit) is exactly the difference between losing 1-in-5,800 docs and
1-in-340,000. This **refines the earlier "u24 required" call**: the right shape is the
NodeWidth pattern — **u16 narrow form for ≥ 99.999 % of docs, per-doc escalation to u24 for
the outliers** (which PR 6's mutable⇒wide machinery provides anyway), not blanket u24.

**Shared-dictionary K-sweep** (Lane A reference coverage, mean over 240 bundles):

| K | 256 | 1,024 | 4,096 | 16,384 | 65,535 |
|---|---|---|---|---|---|
| general web | 12.1 % | 21.9 % | 34.7 % | 48.4 % | 62.5 % |
| wiki (co) | 97.7 % | 97.8 % | 98.2 % | 99.6 % | 100 % |

General web is **logarithmic with no knee** (~+13 pp per 4× slots) — there is no K where it
saturates; wiki saturates already at K=256. Also: reserving K=16,384 in the u16 top range
would leave 49,151 local slots — *below* the observed sym_union max (48,719 ≈ collision).
**Decision: K = 4,096.** Beyond that the dict eats local headroom the tail actually needs,
for ~4 % more archive size (below).

**Lane economics with zstd** (400 k docs, per-bundle zstd-19; topology est. nodes × 15 B):

- Lane B (text + content-attr values): 41.9 GiB → **4.9 GiB (8.5×)** — vs 24× on wiki text;
  heterogeneous content compresses worse, as 0001 predicted.
- Lane A: per-doc raw 2.1 GiB; raw + dict@4096 = 1.4 GiB; **zstd = 494.5 MiB (4.3×)**.
- Archive ≈ Lane A 1.4 GiB + Lane B 4.9 GiB + topology 10.1 GiB = **16.3 GiB**, of which the
  shared dict saves **721 MiB = 4.3 %**. Compressing Lane A outright would beat the dict by a
  further 909 MiB (5.7 %) — but would destroy the index-comparison/bundle-skip property.
- On wiki the verdict flips: raw + dict (272 KiB) **beats** zstd (1.2 MiB) — near-total dedup.

→ **The shared dictionary is a query-speed structure, not a size structure, on general web**
(~4 % size effect); its size case is wiki-shaped corpora. **Topology dominates the
general-web archive (~62 %) post-compression** — the next size lever after PR 3's node-record
shrink is topology packing, not strings.

**Decisions settled on this data (2026-06-11):** K = 4,096 confirmed; index widths =
u16 narrow + per-doc u24 escalation confirmed; the shared dictionary itself **deferred**
(see §7) — the PR sequence proceeds with all symbols doc-local.

### List storage plan (from the index-width data) — shipped for classes in PR 2.5

In the redesign, replace the per-doc linked lists (4 B/entry: u16 value + u15 next-pointer +
head bit) with **contiguous runs in an append-only arena** (2 B/entry + terminator): a node's
class/attr entries are consecutive at parse time, and live mutation already goes through the
rebuild/wide path, so the next-pointer only pays for a property the redesign no longer needs.
Consequences: list storage ≈ halves; the u15 ceiling disappears (capacity = arena length,
addressed by the node-slot width — u16 covers 99.9997 % of docs, u24 escalation the rest);
no bit-packing of unaligned widths (byte-aligned u16/u24 only — unaligned fields defeat the
single-load hot path, cf. the NodeWidth +65 % lesson). A 99.99 % bar is the right *narrow-
width* target, but not a correctness bar: at 10⁹ docs, 0.01 % = 100 k docs lost, so the
escalation path (100 % coverage) stays mandatory.

### PR 2 results — SymbolTable + classes port (2026-06-11)

Shipped (PR #2 branch `feat/symbol-table`, 3 commits): per-document `SymbolTable`
(`StringHeap` + content-sorted permutation, stable `Sym` ids) replaces `ClassStore`; class
lists become a bare `ListVec` of `Sym`s; the rebuild path drives `ListRebuilder` + a
`SymbolTable::rebuilt` compaction; selector class matching resolves once per document
(string → `Sym`) and compares integers per node. Archive format 4 → 5.

- **Size (neutral):** `wiktionary_co.zim` → `.htmlarc` 75,341,336 B (v4) → 75,331,392 B (v5),
  **−0.01 %**. The old sorted `Vec<u16>` class table is replaced 1:1 by the symbol-table
  permutation, so topology/heap bytes are unchanged.
- **Selector matching (the win):** `select class` (`.vector-menu-content` over fr.serrer.html,
  5,589 elements) **−6 %** (−5.3…−6.9 % across runs, p<0.01) — per-node string compare → `Sym`
  compare. `select divs` (class-free) **neutral** (−0.2…−0.8 %, within noise): the resolve
  pass costs nothing when there are no class selectors.
- **No per-node penalty when resolve can't help:** new benches `select absent class` (~110 µs)
  and `select multi-class` (~109 µs) sit level with `select class` — the engine still walks
  every element, so an `Absent` selector only saves the per-node compare, not the walk. The
  document-level prune an absent class enables is **bundle-skip, which is deferred with the
  Lane A dictionary (§7)**; until then `Absent` is correctness (esp. through `:not`), not speed.
- **Everything else neutral-to-better:** parse **−2.6 %** (`build()` drops the list
  value-reindex pass that the old `ClassStoreBuilder` ran); iteration −1…−2.5 % / repack +1.6 %
  / load −1.3 %, all within criterion's noise threshold. 297 dom tests + full workspace green.

### PR 2.5 results — contiguous-run class lists (2026-06-11)

Shipped (branch `feat/class-run-arena`): `RunVec` — one append-only `Vec<u16>` arena where
each class list is a contiguous run of `Sym`s ended by a `0xFFFF` terminator — replaces the
linked `ListVec` backing of class lists (4 B/entry: value + u15 next-pointer/head bit →
2 B/entry + 2 B/list). Node class slots hold the run's start offset directly (no list-table
indirection); matching scans a contiguous slice. The two class ceilings (65,534 lists /
32,768 next-pointer-addressable entries) collapse into one 65,535-slot arena cap (u24
escalation in PR 6). Live mutation extends the trailing run in place, relocates any other
run to the arena end (garbage until repackage — the same GC point that compacts the symbol
table), and emptying a run drops the node's pointer immediately. `ListVec` survives only
inside the attribute stores until PR 3 deletes them. Archive format 5 → 6.

- **Size:** `wiktionary_co.zim` → `.htmlarc` 75,331,392 B (v5) → 74,628,384 B (v6),
  **−0.93 %** — the class-list halving against a topology-dominated archive.
- **Class matching (the win):** back-to-back vs main — `select class` **−5.0 %**
  (113.3 → 107.7 µs), `select multi-class` **−4.6 %**, `select absent class` **−4.5 %**:
  the per-node Sym scan is now a contiguous slice instead of a pointer chase.
- **Everything else neutral** once corrected for machine drift: parse +0.2 %, repack +1.0 %,
  load +0.3 %, `select divs` −1.5 %, iteration −1.6…−2.6 % (mechanism-free; layout/cache
  luck). *Method note:* a **null run** (main re-benched against its own baseline) showed
  parse "−10 %", load "−3.6 %" and `iteration safe` "+11.9 %" on identical code — raw
  criterion change-vs-baseline numbers on this machine include that much drift, so the
  table above compares consecutive absolute times and the null run is the noise floor.
- End-to-end: class-selector probe over the converted v6 archive matches (9,567 hits for
  `.mw-parser-output`); the v6 binary cleanly rejects v5 archives. 303 dom tests + full
  workspace green, snapshots untouched.

### PR 3 results — unified AttrStore (2026-06-11)

Shipped (branch `feat/unified-attrstore`, 5 commits): `AttributeStore` + `DataAttributeStore`
merge into one `AttrStore` — `(NameSym, ValueRef)` entries (values in the store's own
`SymbolTable`, extended names sharing the document symbol table at `NameSym = sym + 256`),
per-element attribute lists as contiguous entry-id runs in the class `RunVec` arena. The
linked `ListVec` (and its `shift_values_from`/`reindex_value` passes) is deleted; the node
record drops its data slot (`node_size` 22→20 / 17→15). `data-*` loses its prefix special
case; unknown attribute names parse as extended attributes; attributes render in source
order. `#id` and attribute selectors join classes in resolve-once integer matching. Archive
6 → 7.

- **Size (the headline):** `wiktionary_co.zim` → `.htmlarc` 74,628,384 B (v6) → 71,561,464 B
  (v7), **−4.1 %** — the per-node data-slot removal against a topology-dominated archive.
  Deterministic (not timing-sensitive). Extended `data-` prefixes are now stored in full but
  are dwarfed by the slot saving.
- **Rebuild / load (clear wins):** `repack` **−7.5 %** (260.0 → 240.4 µs), `loading`
  **−8.8 %** (4.23 → 3.86 µs) — and these came out faster *despite* a heavily contended
  machine (below), so the real gains are larger. `repack` no longer runs the build-time
  list value-reindex pass; `loading` maps a smaller node blob.
- **No regression anywhere; per-bench deltas not cleanly resolvable.** The bench host was
  pinned at load ~7–8/14 cores all session (Chrome, WindowServer, parallel `rustc` from other
  workspaces); `pmset` confirmed *no* thermal throttle and AC power. The effect was a stable,
  uniform **+50 %** on the *unchanged* `iteration` benches — a pure machine-factor floor. Every
  changed bench's branch/main ratio sits **below** that +50 % (parse ≈ +7 %, `select div`
  +39 %, `select class`/`absent`/`multi` +20–23 %), i.e. all faster than the machine penalty →
  none regressed, several improved, but the sub-floor deltas can't be quoted precisely under
  that much contention. The clean figures await a quiet host; the class-select win itself was
  measured at −5 % in PR 2.5 and is unchanged here.
- **id / attribute benches (new):** `select id` 124 µs, `select attr exact` 127 µs,
  `select attr insensitive` 128 µs, `select ext attr` 129 µs — at the same contended level
  as `select class` (130 µs), confirming the resolved id/attr paths walk at class-select
  cost (the integer id scan + name prefilter), not the old per-node string scan.
- End-to-end: `.mw-parser-output` class probe parity (9,567 hits) on the v7 archive; a
  `[data-mw]` attribute probe resolves; the v7 binary rejects v6 archives. 302 dom tests +
  full workspace green; the only snapshot change is attribute reordering (verified as a pure
  reorder — every line's attribute multiset is preserved, no content delta).

### PR 4 results — extended tags (2026-06-11)

Shipped (branch `feat/extended-tags`, 3 commits): unknown/custom tag names stop being parse
errors and are stored in a per-document `ExtTags` vocab encoded in the node's 1-byte tag —
`[0,192)` enum discriminants, `[192,255)` vocab indices (≤63 `Sym`s into the shared symbol
table), `255` an overflow sentinel resolved via a node-index-keyed side map. `nodes.tag()`
normalizes any byte ≥ `EXT_BASE` to a new `HtmlTag::extended` marker (absent from every
classifier, so custom elements are non-void / non-raw-text / never-auto-close); `tag_name()`
resolves the real string. `hnan`/`figure_inline` are demoted out of the enum. The `DomStack`
tree-builder gains an associated `Tag` type carrying full identity (a `Sym` in the builder, a
`String` in the test DOM) so two distinct custom elements — which share the `extended` kind —
never close one another. Extended tag selectors join classes/id/attrs in resolve-once integer
matching (`ExtTagSelector` → vocab byte / overflow sym / `Absent`). Archive 7 → 8.

- **Size (small, deterministic increase):** `wiktionary_co.zim` → `.htmlarc` 71,561,464 B (v7)
  → 71,877,824 B (v8), **+0.44 %** (+316 KB). The cost is the per-document `ext_tags` field —
  two rkyv `Vec` headers on every doc even though almost none hold custom elements — plus the
  two demoted MediaWiki tags (`hnan`, `figure-inline`), which moved from a free enum byte to a
  symbol-heap string + a vocab entry. Anticipated and accepted: it buys storing custom
  elements that previously failed the whole document, and is dwarfed on general web where
  custom elements are common (and, on main, fatal).
- **Parse neutral.** Branch-vs-`main` on `parse fr.serrer.html`: **−0.27 % (p = 0.49, "no
  change")** — the standard-tag path is unchanged work (`from_tag_name` wraps the same
  `HtmlTag::try_from`; node creation goes through the byte twin of `add_as_last_child`). The
  *unchanged* `iteration` bench swung **−10 %** between captures on identical code, the
  documented host drift (see [[bench-host diagnosis]]) — so finer deltas are unquotable, but
  parse's own comparison is squarely neutral.
- **Extended tag selectors (new, informational — no `main` baseline; they were parse errors
  before):** `select absent ext tag` 134 µs on the wiktionary fixture (≈ `select divs` 135 µs
  — the `Absent` prune is as cheap as a standard-tag walk); `select ext tag (vocab byte)`
  173 µs over a ~4,000-element custom-element doc (a single per-node tag-byte compare).
- **The rebuild trap, handled (the one real design hazard):** extended tag *names* share the
  symbol table with class tokens and extended attr names, so the rebuild adds a tag-name union
  pass (the twin of PR 3's attr-name pass) marking live custom-element name syms before the
  table compacts; the vocab is then re-derived from scratch via the shared `encode`,
  auto-compacting freed slots and rewriting the node-keyed overflow map. Pinned by a
  repackage-keeps-extended-tag-names regression test and an overflow-vocab re-derive test.
- **The silent-corruption trap, fixed:** the width-`repack` now copies the raw tag byte, not
  `tag() as u8`, which would have collapsed every extended byte to the `extended` marker on the
  U24→U16 down-pack (only at serialize time, far from parse tests). Pinned by a U16 archived
  round-trip over a custom-element document.
- End-to-end: `.mw-parser-output` class probe parity (9,567 hits) on the v8 archive; the v8
  binary rejects a forged-v7 archive. Full workspace green (372 tests); **zero snapshot
  churn** — no fixture contains a custom element, and the demoted MediaWiki tags appear in
  none, exactly as predicted.

**Decisions recorded:** (1) the rebuild re-derives the vocab from scratch rather than keeping
it stable — one code path shared with parse, free compaction, and the overflow map must be
rewritten anyway. (2) Reserved spellings (`extended`, `text`, `comment`, `doctype`) route to
extended tags, so `<text>` is a custom element rather than a malformed system node — a small
correctness fix bundled in. (3) Unknown CSS tag selectors stop being parse errors (the
selector-side mirror), matching nothing unless the document holds that element. (4)
`CompoundSelector.element: Option<HtmlTag>` was kept and a separate `ext_element` field added,
rather than widening `element` to an enum — the latter would have churned 102 standard-tag
construction sites across the test suite for no behavioural gain.

### PR 5 results — foreign content (2026-06-12)

Shipped (branch `feat/foreign-content`, 3 commits): the tokenizer's `<svg>`/`<math>` skip
machinery (`Driver.skip`/`skip_awaiting_close`/`StartTag::Foreign` + eight `skip.is_some()`
early-returns) is deleted — foreign subtrees now parse as ordinary elements through the PR 3/4
extended attr/tag machinery. `RawTextEmitter` gains a name-based foreign-content depth:
inside svg/math the raw-text state switch is suppressed (so `<style>`/`<title>`/`<script>`
children parse as markup) and `<![CDATA[…]]>` is tokenized as character data via the
`adjusted_current_node_present_but_not_in_html_namespace` emitter hook rather than a bogus
comment. A new `html/foreign.rs` holds the WHATWG "adjust SVG tag names" (37), "adjust SVG
attributes" (58), and MathML `definitionURL` tables — sorted, binary-searched, applied
context-free at the four formatter tag-name emit sites and the extended-attr-name emit site;
stored names stay lowercase so the symbol table and selectors are case-stable. Extended tag
selectors became ASCII-case-insensitive. Formatters mirror the parser: script/style inside
foreign content are entity-encoded, not emitted verbatim. **No archive format bump — v8 layout
is unchanged**; this is a parser/formatter behaviour change.

- **Size — wiktionary_co.zim: byte-identical**, 71,877,824 B (v8) → 71,877,824 B, **0.00 %**.
  The corpus contains no svg/math and no recoverable mismatches, so there is nothing to store
  or repair; the deterministic equality confirms zero regression on the wiki path.
- **The skip machinery was masking a tree-builder gap.** Once svg/math are parsed, the common
  real-world icon pattern `<svg>…<path></svg>` (a `<path>` left open — no `/`, no `</path>`)
  reached the strict `pop_tag`, which failed the **whole document** on the mismatch. On a
  Common Crawl sample (`cc_000`, 34,465 HTML docs) this lost **228 net docs** vs main (329 newly
  failing, 101 newly passing); **98 % of the new failures (323/329) erred inside an svg subtree**
  (`… > svg > path`, `… > svg > symbol > path`). Storing previously-dropped content must not
  *reduce* extraction coverage (extraction is the product), so PR 5 grew a third commit:
- **Tolerant end-tag recovery** (`DomStack::pop_tag` + a non-destructive `_stack_contains`):
  when an end tag matches an element open *deeper* in the stack, the intervening unclosed
  elements are popped (implicitly closed) — what a real foreign-content/HTML tree builder does —
  instead of erroring. A stray end tag with no open match still errors, so genuine corruption is
  not masked. It runs **only on the path that previously errored**, so every document that
  already parsed is byte-identical (zero snapshot churn, wiktionary_co unchanged). The fix is
  **general**, not svg-specific: on `cc_000` it raised conversions **21,455 → 25,790 (+4,335,
  +20.2 %)**, failures **13,010 → 8,675 (−33 %)** — recovering thousands of pre-existing
  misnested-HTML failures alongside the foreign-content ones.
- **Size on `cc_000` (general web):** 2,798,471,832 B → 3,527,265,712 B, **+26.0 %** — but the
  branch stores **20 % more documents**, so the total is dominated by coverage, not per-doc
  bloat. Per *converted* document: 130,434 → 136,769 B, **+4.9 %** (the stored foreign subtrees
  plus the messier newly-recovered docs). Storing previously-dropped content is the intended
  product change.
- **Parse neutral.** `parse fr.serrer.html` branch 1.289 ms vs main 1.319 ms (−2.3 %, within
  host drift) — the parse-path additions are a per-tag `svg`/`math` byte-slice compare and a
  foreign-depth check; recovery and the case tables sit on the error and format paths, neither
  of which the parse bench exercises. No formatter bench exists, so the case-table lookups are
  ungated (a binary-search miss per extended attr/tag name); they are off every measured path.
- **Tests:** svg/math/CDATA/self-close round-trips, the title-RCDATA-vs-foreign asymmetry,
  case restore + ASCII-case-insensitive selectors, unclosed-child recovery (incl. a general
  `<div><span></div>` case) and the still-errors-on-stray-end-tag guard, an svg-subtree
  repackage-survival test, and a realistic svg+math fixture across the round-trip/pretty/
  repackage/remove-formatting/select globs. 339 dom tests + full workspace green.

**Decisions recorded:** (1) **No WHATWG foreign-content breakout / integration-point rules**
(`<svg><div>` nests the div inside svg) — an accepted deviation for a fault-tolerant extractor;
the tolerant end-tag recovery, not breakout, is what keeps coverage from regressing. (2)
Foreign-content depth is **name-based, not a namespace stack** — an unclosed `<svg>` leaves the
flag set until EOF, and CDATA inside a `<foreignObject>` HTML subtree is still treated as CDATA;
accepted as a rare, bounded approximation. (3) CDATA section markers are **dropped** (the
content is kept as text) — semantic extraction, not byte-verbatim archival. (4) Case adjustment
is **context-free by name** (a stray `<clippath>` outside svg also renders `clipPath`), matching
the spec's own name→name tables and keeping storage/selectors case-stable. (5) Recovery is
**general end-tag repair**, not gated to foreign content — chosen over a foreign-only variant
because it needs no builder-side foreign-context tracking and the broad coverage gain (+20 % on
general web) is a pure win, with stray end tags still erroring.

**Build note (not part of the format work):** the workspace `cli/*` member glob was failing
because `cli/zim2htmlarc` was an orphaned data-only directory (the crate was renamed to
`htmlarc-convert`; the dir held only local, gitignored ZIM/WARC fixtures and no manifest). Its
`testdata/` was relocated to `cli/htmlarc-convert/testdata/` (the path the convert e2e test
already expects) and the empty `cli/zim2htmlarc` removed, so the glob loads with no workspace
change.

### PR 6 deferral — ref widths measured per-document (2026-06-15)

A `WidthImpact` probe (joint cross-tab + a topology-byte model of both candidate node layouts)
added to `htmlarc-convert stats` was run over the same 60-segment Common Crawl corpus as the
PR 1 gate (2,041,140 docs, 240 bundles). It settles the node-record width policy and **defers
PR 6**.

**The reference widths are per-*document*, not per-bundle or per-corpus.** `sym_union`,
`distinct_pairs`, and `list_entries` are single-document cardinalities (a fresh counter per
doc); each document carries its own `SymbolTable` / `AttrStore` / `RunVec`, u16-indexed. A
document needs u24 iff *it alone* exceeds the u16 ceiling — bundle size never enters. The
earlier "grows with corpus size" wording was the extreme-value effect of sampling a fixed
per-doc distribution: drawing more docs eventually *includes* a worse tail document, rather
than any document growing. (The deferred Lane A shared dict, §7, is the only bundle-scoped
symbol structure, and it *relieves* per-doc local pressure — bundle-common symbols leave the
local range — so bundling pushes away from per-doc u24, never toward it.)

**What actually crosses u16 at 2 M docs** (all that PR 6's u24 work would serve):

| ref | per-doc max | docs > u16 | status |
|---|---|---|---|
| node links (`nodes`) | 145,797 | 40 | already handled — `NodeWidth::U24` |
| RunVec arena offset (`list_entries`) | 84,452 | **6** | the only new u24 need |
| `SymbolTable` (`sym_union`) | 48,719 (74 %) | **0** | no crossing |
| `AttrStore` entries (`distinct_pairs`) | 50,935 (78 %) | **0** | no crossing |

**Node-record width — single vs mixed.** Joint crossing of the two node-record axes (link
slots vs the class/attr arena-offset slots):

| | `list_entries` ≤ u16 | `list_entries` > u16 |
|---|---|---|
| `nodes` ≤ u16 | 2,041,094 | 6 |
| `nodes` > u16 | 40 | **0** |

The two overflows are **disjoint** — zero documents cross both (node-heavy vs attribute-heavy
are different pathologies). Mixed-width (independent link/ref width → 15/17/20/22 B) over
single-width (one width → 15/22 B) saves **7.0 MiB on 50.4 GiB topology = 0.0136 %** — not worth
a 2×2 layout matrix, 2-D offset accessors, and extra hot-path arms (cf. the NodeWidth +65 %
single-load lesson). **Single-width settled** — but moot under the deferral: with no u24 ref
slot the node record stays today's 15 B / 20 B.

**Decision — defer PR 6** (the u24-refs work and its dependent mutable⇒wide cleanup), for the
same shape of reasons as §7's PR 7 deferral:

- It serves **6 documents** (arena) + **0** (symbols/pairs) on 2 M docs. PR 1's checked inserts
  already make those 6 a **clean per-doc skip, not corruption**, so deferral is a coverage
  choice on 0.0003 % of (pathological utility-CSS) docs, not a correctness risk.
- **Not a dependency:** Lane B, topology packing, and the shared dict need nothing from u24
  refs. The width flag + per-store down-pack land at a later format bump (cheap pre-1.0,
  clean-slate), exercised against a real overflowing doc rather than a 0-doc path.
- Deferring u24 **dissolves most of PR 6**: the node single/mixed question is moot (record
  unchanged), `into_optimal_width` has no second store-width to generalize, and `DOWNPACK_MARGIN`
  removal alone is not worth doing. The one separable remnant — a u16 doc loaded then edited
  >6,500 nodes past the margin — is a latent, rare *node-only* overflow, a ~10-line
  widen-on-repackage fix if it ever bites, not a PR.

**Re-evaluate when** a real document first crosses the `sym_union` / `distinct_pairs` / arena
u16 ceiling (the per-doc max is climbing — `sym_union` 7,090 → 32,229 → 48,719 across
8 k → 136 k → 2 M docs), or alongside topology packing — whichever comes first. Per the lane
economics, **topology (~62 % of the post-compression archive) is the next real size lever**, not
ref widths.

### Topology packing measured — parked behind a hot-path-safe encoding; parser recovery outranks it (2026-06-15)

The `stats --topology` probe (parses each document through the production path
`parse → into_optimal_width`, then walks the node blob; logic verified by a hand-computed unit
test) ran over the same 60-segment Common Crawl corpus. It measures the redundancy in the five
node-*link* slots — the size lever the lane economics named.

**Findings (1,548,715 parsed docs, 2.51 B nodes):**

- **Links are 52 % of the topology blob** (18.2 of 35.1 GiB), not the ⅔ that "10 of 15 B/node"
  implies: text/comment nodes are 56 % of all nodes and spend 8 bytes on the `u32` text range
  instead of two link slots, diluting the link share.
- **The blob is already clean document order.** `dead = 0`, and the serialized blob is
  *byte-identical* to a `rebuild()`-ed one — the builder appends in document order and the
  convert path leaves no dead slots. A reorder/compaction pass buys **nothing** (−0.0 %); the
  only lever is packing the links.
- **Document-order locality is extreme.** Each link's delta from the node's own index, as a
  zigzag-varint width over present links: parent 84.5 % 1 B / 99.4 % ≤ 2 B; prev/next ≈ 97 %
  1 B; first 100 % 1 B; last 94.9 % 1 B. `first_child == self+1` for **79.9 % of elements**.
- **Ceiling: −26.2 % of the topology blob** (35.1 → 26.0 GiB) — varint deltas + implicit
  first-child + a 1 B/node presence mask; the link bytes alone go 18.2 → 6.7 GiB (−63 %).

**Why parked, not picked up:**

1. The −26 % is a **varint** ceiling, and variable-width fields defeat the single-load traversal
   hot path (the NodeWidth +65 % lesson). The hot-path-safe subset — implicit first-child (a
   fixed 13 B element record + a side table for the 20 % non-implicit), with dead slots already
   at 0 — captures only part of it. The full win needs a fixed-1 B-delta-with-escape design
   (prev/next/first/last are 95–100 % one byte; parent is the holdout at ~15 % needing 2 B) that
   is not yet designed or benched. The *size* is proven; the *speed-safe encoding* is the open
   work — the same measured-but-parked state as PR 6 / PR 7.
2. **A bigger lever outranks it.** The same probe found **24.1 % of the corpus's text/html
   documents (492,425 of 2,041,140) fail `HtmlDoc::parse` and are dropped from the archive
   entirely** — see [0003](0003-parser-error-recovery.md). Reclaiming a quarter of the corpus
   beats shrinking the kept three-quarters by 26 %, so the next format work waits behind parser
   recovery.

**Re-evaluate** alongside Lane B (PR 8 — same format bump), once a hot-path-safe link encoding
exists and parser recovery has settled how many documents the format must hold.

## Open questions

- ~~Reserved low `ValueRef` specials (e.g. interned-empty for boolean attrs)~~ — **decided
  PR 3: no specials.** An empty value (`disabled`, `data-x`) interns as an ordinary value
  sym (the status quo for `class=""`); only the formatter's `val.is_empty()` check and
  `Attribute`'s `Display` consume emptiness, both on the deref'd `&str`. No reserved range
  needed.
- WHATWG first-wins for duplicate attribute names (`<a id="x" id="y">`) — PR 3 keeps today's
  behaviour (html5gum's callback emitter streams duplicates; only distinct `(name,value)`
  pairs dedup, so both render). First-wins would need a per-element seen-names set in the
  tokenizer `Driver`; low priority, no consumer is known to depend on either behaviour.
- Resolve attribute *values* (not just names) to entry ids for the `Exact`+case-sensitive
  case — PR 3 resolves the name to a `NameSym` (integer prefilter) and keeps the value
  compare string-based. Full entry resolution would save one string compare on the (usually
  ≤1) name-matching entry; measure whether it is worth the extra resolve state.
- Exact `EXT_BASE` vs. enum-promotion budget — after PR 1 probe + PR 5 corpus data.
- Lane B framing (per-bundle frame vs per-doc blobs + trained dict) — PR 8, per 0001. The
  8.5× general-web ratio (vs 24× wiki) makes the per-bundle-trained-dictionary variant more
  attractive for heterogeneous corpora.
- Topology packing for general web (the ~62 % post-compression share) — **measured 2026-06-15**
  (`stats --topology`): −26.2 % topology-blob ceiling via varint + implicit-first-child links,
  but that ceiling is variable-width (hot-path cost), and a reorder/compaction pass is a no-op
  (`dead = 0`). **Parked** behind a fixed-1 B-delta-with-escape design and behind parser
  recovery ([0003](0003-parser-error-recovery.md)); see the topology subsection above.
- `RunVec` terminator → side bitvec (possible future optimization, analyzed 2026-06-11 and
  parked). Replacing the 2 B/run `0xFFFF` terminator with a 1-bit-per-slot boundary bitvec
  would save ~0.39 % of the archive on wiktionary_co (terminators are 361,110 B = 25.3 % of
  the class arena; the bitvec would be 66,523 B). Parked because: (a) it adds a second
  dependent load to the per-node match scan — the exact path that produced PR 2.5's −5 %
  class-select win (cf. the NodeWidth +65 % lesson on breaking the single-load pattern);
  (b) in-place remove/relocate must shift bits in lockstep across byte boundaries —
  complexity on an already-subtle mutation path; (c) Lane B per-bundle zstd (PR 8) will
  compress the highly regular terminator pattern to near-zero, so the raw saving mostly
  evaporates post-compression. Re-evaluate with real numbers when PR 3 moves attribute
  lists onto `RunVec` (a several-times-larger arena, so terminators grow in absolute
  bytes), and only if a post-Lane-B measurement still shows them as a real cost.
- Archive key index (`htmlarc-archive` doc table, outside this ADR's stores): `fst::Map`
  measured **4.6× smaller** than the current keys + 4 B/doc permutation on wiktionary_en
  (8.87 M keys: 199 MiB → 43 MiB) with **1.6× faster** exact lookups (332 vs 544 ns) and
  free prefix/range/regex queries — but only 1.4× on prefix-poor cross-web key sets
  (343 k CC URLs ≈ 1.2 pages/host). Requires byte-lexicographic order (drops the grapheme
  `key_len` dimension, which has no other consumer, and the `unicode-segmentation` dep);
  positional `entries()`/`list` need key-by-position, so either stream the fst at load or
  keep keys in the doc table and replace only the permutation (smaller win). Decide at the
  PR 8 archive-format bump.
