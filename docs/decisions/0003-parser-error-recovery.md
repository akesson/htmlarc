# 0003 — Parser error-recovery: closing the text/html strictness gap

- **Status:** Proposed (direction accepted; approach open)
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
