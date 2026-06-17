# 0003 — Parser error-recovery: closing the text/html strictness gap

- **Status:** Accepted — implemented (rounds 1–2); fork resolved as **(b) harden the in-house
  builder**. Confirmed at full corpus scale: **24.1 % → 0.00054 % document loss** (11 of
  2,036,318 docs), all 11 genuine capacity overflow — zero structural.
- **Date:** 2026-06-15
- **Scope:** `htmlarc-dom` (parser / tree builder), `cli/htmlarc-convert` (skip accounting)
- **Companion:** surfaced by the [0002](0002-unified-symbol-stores-extended-names.md)
  `stats --topology` probe; reprioritizes 0002's remaining size work behind it.

## Context

The converter parses each `text/html` response with `HtmlDoc::parse` and **drops any document
that fails**, emitting one stderr line and incrementing a "Failed:" tally (`convert.rs` — the
code comment calls it "the strictness gap"). The WARC reader has already filtered to
`text/html` `response` records, so these are real HTML pages, not non-HTML noise.

Measured over the 60-segment Common Crawl corpus (2,041,140 text/html docs — the same gate
corpus as 0002):

> **492,425 documents — 24.1 % — fail to parse and never enter the archive.**

**This is not the 0002 capacity ceilings.** On a representative 8,000-doc segment, of 2,160
failures:

| class | share |
|---|---|
| structural — "Expected tag `X`, but found stack `Y`" | 97.9 % |
| "Closing a tag, but none open" | 2.1 % |
| capacity overflow (depth / node / list caps) | 0.05 % (1 doc) |

The tree builder is strict exactly where the HTML5 parsing algorithm mandates *recovery*.
Proven minimal reproduction, run through the actual `convert` binary:

| input | result |
|---|---|
| `<div id="x"/></div>` | **FAILS** — "Expected tag 'div', but found stack 'html > body'" |
| `<div id="x"></div>`  | converts |
| `<img><br><p>…</p>`   | converts (well-formed void elements are fine) |

`<div/>` is XML self-closing syntax on a non-void HTML element. HTML5 ignores the `/` and treats
it as `<div>`, so the later `</div>` matches; htmlarc self-closes it, the `</div>` is then
unmatched, and the **entire page is discarded**. A real 1.8 KB page in the corpus dies on
exactly this. The dominant failing end tags span void elements (`br`, `input`, `link`, `track`,
`wbr`, `img`, `embed`, `meta`) and ordinary mis-nesting (`div`, `p`, `a`, `li`, `td`) — the set
of constructs the HTML5 tree-construction "anything else" / adoption-agency / implied-end-tag
rules exist to absorb.

This is the concrete cost of the standing **html5gum-vs-harden-our-own-builder** fork: htmlarc
runs its own tree builder over html5gum's tokenizer, and that builder does not implement HTML5
error recovery.

## Decision

**Prioritize parser error-recovery as the next workstream, ahead of further storage-format size
work.** For an extraction-first archive, a 24 % document-loss rate dominates a 26 % shrink of the
documents that *are* kept (0002's topology ceiling). Topology packing stays measured-and-parked
in [0002]; Lane B (PR 8) follows recovery.

**The approach is intentionally open.** Constraints and the proposed first step:

- **Goal:** never drop a document the HTML5 parsing algorithm can build. For structural
  mismatches the builder must always produce a tree, never an `Err`; reserve hard failure for
  genuinely unrecoverable input, and even then prefer a best-effort partial tree to a drop.
- **Decide the fork on data.** Either (a) move to spec-complete tree construction (html5gum's,
  if it exposes one, or another spec builder), or (b) harden the in-house builder with the
  specific recovery rules the corpus demands. (b) is incremental and preserves the existing
  store/extraction pipeline; (a) is more complete but a larger swap that must still fit the
  extraction model. The failure-bucket data below should drive the choice.
- **First step — a corpus-wide failure-bucket probe.** `convert` already emits a per-document
  reason; bucket all ~492 k failures by error class and triggering construct, and estimate how
  much of the 24 % a small rule set reclaims (self-closing slash on HTML elements, stray
  `</void>` / orphan end tags, implied end tags, foster-parenting of mis-nested table content).
  The 8 k-doc sample suggests a handful of rules covers most of it; confirm at corpus scale
  before committing to (a) vs (b).

## Consequences

- The archived document population grows **~+32 %** (492 k recovered / 1.55 M kept today) once
  recovery lands. These are still per-document trees, so they add no new 0002 ceiling pressure,
  but Lane B and topology sizing should be re-measured on the *recovered* population.
- Recovery changes the stored tree for malformed inputs (it builds what is currently dropped);
  the convert "Failed:" tally becomes the regression metric to drive toward zero.
- Parser-side only — no archive format bump required to start.

## Implementation & results

The fork was decided on data as planned: **(b) harden the in-house builder**, in two rounds.
Both were measured by an A/B through the real `convert` binary (two release builds, identical
input) over a representative **343,427-doc / 10-segment** Common Crawl slice
(`HTMLARC_CONVERT_THREADS=8`); the strict baseline's 24.80 % loss lands on the 24.1 %
full-corpus figure, so the slice is representative. Recovery is verified **strictly monotonic**
each round — the set of post-change failure keys is a strict subset of the pre-change set, so no
document that parsed before ever regresses.

| stage | failed / 343,427 | corpus loss | reclaimed |
|---|---|---|---|
| strict baseline (pre-0003) | 85,155 | 24.80 % | — |
| after **round 1** | 66,552 | 19.38 % | 18,603 (21.8 % of failures) |
| after **round 2** | **55** | **0.016 %** | 66,497 (99.92 % of the round-1 tail) |

Across both rounds, **85,100 of the original 85,155 failures (99.94 %) are reclaimed** — the
strictness gap is effectively closed. The reclaimed documents are genuinely archived (unique
docs in the slice's archive rose 276,875 → 343,372, a delta of exactly 66,497), not merely
un-errored.

**Confirmed at full corpus scale (all 60 segments, 2,036,318 docs, recovery parser, 5m 27s).**
The slice projection holds: **11 documents fail = 0.00054 %** (≈ 5 per million), down from the
strict baseline's **24.1 % (492,425 docs)** — **≈ 99.998 % of the strictness gap closed over the
whole corpus**, not just a representative slice. Every one of the 11 is genuine per-document
capacity overflow, **zero structural**:

| residual class | count |
|---|---|
| class-list arena (> 65,535) | 4 |
| attribute-list arena (> 65,535) | 4 |
| element nesting (> 8,192) | 3 |

The 3 nesting failures are notable: the slice had none (its deepest document was 3,235), so the
full corpus surfaced three genuinely pathological deep-nested pages beyond the 8,192 sanity
ceiling — the cap catching real nesting bombs exactly as intended, rather than ordinary deep
pages (which `TinyVec` heap-spill now absorbs). The parser fails **only** on the fixed-capacity
store ceilings, corpus-wide.

**Round 1** (commit `72d9c34` foreign-content stack-walk on `main`, then `642a50b`): ignore a
self-closing `/` on ordinary/custom HTML elements (`<div/>`→`<div>`); make `<source>` void;
ignore stray `</void>` end tags; and the foreign-aware deeper-stack-walk that closes unclosed
ancestors (e.g. `<svg>…<path></svg>`).

**Round 2** (this change, `crates/htmlarc-dom/src/parser/dom.rs` `pop_tag`): **an end tag that
matches no currently-open element is ignored, not fatal** — whether the stack is empty or the
tag is open nowhere on it. This implements HTML5's "drop an unmatched end tag and keep building"
for the two error sites that were the entire remaining structural tail (97.4 % mismatched-end-tag
+ 2.5 % stray-end-tag). Membership is probed *before* popping, because the builder's pop is
destructive and re-pushing would duplicate a node. It **supersedes** the earlier ADR 0002 PR 5
stance (`stray_end_tag_with_no_open_match_still_errors`) that kept such tags fatal "to surface
structural corruption" — the wrong trade for an extraction archive, where it cost ~20 % of the
corpus. With no adoption-agency algorithm, an ignored orphan can occasionally reparent following
content versus a full HTML5 builder, but the document is *extracted* rather than *dropped*.

**Residual (55 docs, all genuine capacity overflow — zero structural):** 53 nesting-depth
(> 256), 1 class-list arena, 1 attribute-list arena. The parser now fails **only** on the
fixed-capacity store ceilings, exactly the stated goal ("reserve hard failure for genuinely
unrecoverable input"). Nesting-overflow failures rose 42 → 53 between rounds: ~11 deeply-nested
documents now parse *past* their first structural orphan and only then hit the 256-depth ceiling
— a bucket shift, not a regression.

### Round 2b — spill-to-heap parse depth (depth cap resolved)

Probing the 53 nesting failures with an uncapped build showed their true depths cluster tightly:
264–3,235 (median 380, p90 842) — ordinary deeply-nested real pages (e-commerce listings,
nested-quote forum threads), **none pathological, none over 8,192**. So the fixed depth cap, not
the documents, was the limit.

The parse stacks (`tag_stack` / `index_stack` in `DomBuilderCursor`) moved from a fixed
`ArrayVec<[T; 256]>` to a **`tinyvec::TinyVec<[T; 256]>`**: the first 256 levels stay inline
(no heap, identical to before for ~99.8 % of documents), and deeper nesting **spills to a heap
`Vec`** instead of being dropped. The hard cap became an 8,192 *sanity* ceiling (above any real
document; `MAX_NODES` backstops a true nesting bomb). This is the builder.rs `MAX_DEPTH` TODO,
but keeping inline storage for the common case rather than the TODO's plain-`Vec` (which would
heap-allocate every parse). Cost, isolated by microbench (`ArrayVec`+fast-path vs `TinyVec`+
fast-path): **~1.6 % parse** from the inline-path enum branch — and the same change folds in an
O(1) end-tag fast path (`pop_tag` checks the stack top before the linear `_stack_contains` scan
round 2 had put on every close).

Re-measured on the same slice: failures **55 → 2** (the two u16-arena overflows;
`mesicnikosmicka.cz` class-list, `xopenload.me` attribute-list) = **0.0006 % loss**. Nesting is
no longer a document-loss cause. The remaining 2 are the genuine per-document store ceilings.

## Re-measure on the recovered population — 0002 constants frozen

The re-measure that round 2b deferred is done: `stats --compress --topology` over the same
343,427-doc / 10-segment slice, where the recovery parser now builds **343,665 of 343,667**
documents (the topology probe runs over essentially the whole population for the first time;
the two failures are the u16-arena docs). The **+24 % recovered documents did not move a single
sizing economic** — every figure the 0002 constants encode held:

| metric | parked baseline (~76 % parsed) | recovered population (99.9994 %) |
|---|---|---|
| dead topology slots | ≈ 0 % | 0.00 % |
| `rebuild()` alone | ≈ no-op | −0.0 % |
| varint links + implicit-first-child + 1 B mask | −26 % | −26.2 % |
| Lane B (text + content attrs), zstd-19 | — | 36.3 GiB → 4.3 GiB (8.5×) |
| Lane A: zstd vs raw + shared dict @K=4096 | zstd wins on bytes | zstd 451.9 MiB beats raw+dict 1.2 GiB by 766.5 MiB |
| node record mixed u16/u24 (PR 6) | negligible | 8 docs need u24 links (0.0023 %), saves 0.0145 % |

The per-link delta-width histograms and the structural invariants (first_child==self+1 79.6 %,
next==self+1 37.4 %, parent==self-1 35.4 %) are indistinguishable from the pre-recovery
baseline: the reclaimed malformed documents are **topologically ordinary**. Topology
(6.2 GiB packed / 8.4 GiB today) still dominates Lane B (4.3 GiB), so the size ordering this
ADR set — topology parked at the −26 % hot-path-safe ceiling, Lane B (PR 8) as the next size
lever — is unchanged.

Every per-document cap was validated. The hard caps (nodes > 16 M, distinct attr pairs,
per-doc symbols) have **zero** documents over them; the populated "over-cap" counts
(`max_depth`, `ext_tag_names`, `list_entries`) are all *graceful spill* thresholds, not drops —
depth spills to the heap stack, extended tags spill to the overflow side map — confirmed by the
parse losing only the 2 genuine arena overflows. **The 0002 capacity constants are therefore
frozen at their current values; the next storage work is Lane B (PR 8).**

A reporting note: `stats` measures `max_depth` with a raw tokenizer that does not auto-close, so
its figures (worst 38,510, 1,329 docs > 256) wildly overcount the real tree-builder stack, whose
effective depth tops out near 3,235. Its `DEPTH_CAP` reference was corrected from the stale
inline threshold 256 to the real 8,192 hard cap, and the naive measure is now commented as a
ceiling probe.

### Deep nesting and traversal safety

Round 2b lets documents nest up to the 8,192 hard cap (real depths reach ~3,235) where the old
build dropped them past 256. This is safe for iteration and CSS queries because **none of the
depth-scaling paths recurse on the native stack**:

- **Descendant iteration** (`ElementIter`) walks an explicit heap-backed `VisitedStack`
  (a `TinyVec` that spills past its inline slots), not recursion — an 8 k-deep document costs
  ~64 KB of heap, no call-stack growth.
- **Ancestor / child / sibling iteration** (`RelativeIter`) is an iterative cursor walk that
  tracks signed depth in an `i16`. The builder's 8,192 cap sits ~4× under `i16::MAX` (32,767),
  and a `const` assert in `builder.rs` now pins that coupling so raising `MAX_DEPTH` past
  `i16::MAX` fails to compile rather than silently overflowing.
- **CSS combinator matching** (`ComplexSelector::verify`) recurses on the number of selector
  segments (user-supplied, tiny), not on tree depth; the depth-scaling ancestor/sibling walk
  inside each segment is the iterative `RelativeIter`. A descendant combinator on a very deep
  document is *slower* (an O(depth) ancestor walk per candidate) but bounded and correct — the
  same per-hop cost that existed at depth 256.

(Orthogonal and pre-existing: sibling ordinals are counted in `u16`, so a node with > 65,535
*direct children* — breadth, not depth — would overflow that counter. Untouched by this change
and not observed, but noted.)
