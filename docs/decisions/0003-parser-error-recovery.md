# 0003 — Parser error-recovery: closing the text/html strictness gap

- **Status:** Accepted — implemented (rounds 1–2); fork resolved as **(b) harden the in-house
  builder**. Rounds 1–2 close ~99.9 % of the gap; remaining failures are capacity overflow only.
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
— a bucket shift, not a regression (the depth cap, builder.rs `MAX_DEPTH`, has a standing TODO to
move to a heap `Vec` with a ~8,192 sanity cap, which would reclaim most of these too).

**Next:** re-measure Lane B (PR 8) and topology sizing on the now ~+24 %-larger archived
population before freezing the 0002 constants.
