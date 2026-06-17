# 0004 — Bundle size: 1,000 documents

- **Status:** Accepted
- **Date:** 2026-06-17
- **Scope:** `crates/htmlarc-archive` (`BUNDLE_CAP`), `cli/htmlarc-convert` (convert peak memory)
- **Companion:** sizes the bundle that [0001](0001-string-storage-lanes.md) compresses and
  [0002](0002-unified-symbol-stores-extended-names.md) shares symbols within; the
  per-bundle data region those lanes will populate is still reserved (empty) on disk.

## Context

`BUNDLE_CAP` was 10,000. It controls **two unrelated things at once**:

1. the **convert-time in-flight unit** — a worker accumulates one whole run's serialized
   documents before the coordinator writes them, and the reorder buffer holds up to
   `in_flight_cap` (≈ `2 × cores`) finished runs. Peak RSS ≈ `in_flight_cap × per-bundle bytes`.
   At 10k that is the dominant term (~25 GB of ~26 GB worst case on a 14-core box).
2. the **on-disk compression / symbol-sharing window** — bundles are the unit over which the
   (future, [0001]/[0002]) Lane A shared dictionary and Lane B zstd will amortize.

A smaller bundle is a large, direct memory win, but only if it does not meaningfully cost
on-disk size. **The compression cost is not yet observable** — the per-bundle data region is
reserved but empty (`writer.rs`, every `BundleDesc.data_offset/len = 0`), so today shrinking the
bundle changes essentially nothing on disk except a slightly larger bundle table. The cost is a
bet on the Lane A/B work that hasn't landed. We measured that bet on real data first.

### Measurement (spike)

A throwaway harness (`cli/htmlarc-convert/src/stats/framing_spike.rs`, an `#[ignore]`d test)
extracts each document's real Lane A / Lane B bytes (via the `stats` counting pass) for one 10k
bundle of `cc_000.warc.gz` and compresses them four ways at zstd level 19 (the [0001] reference
level): one frame per bundle, one frame per 1,000 docs, one frame per doc, and per-doc against a
110 KiB per-bundle trained dictionary.

**Lane B (text + content-attr values) — 801.6 MiB raw, 9,965 docs:**

| framing | size | ratio | vs 1-frame |
|---|---|---|---|
| one frame / bundle | 102.4 MiB | 7.83× | baseline |
| **one frame / 1,000 docs** | 102.9 MiB | 7.79× | **+0.5%** |
| one frame / doc | 145.3 MiB | 5.52× | +41.9% |
| per-doc + trained dict | 128.9 MiB | 6.22× | +25.9% |

**Lane A (class/id/searched/ext names) — 46.5 MiB raw, 9,896 docs:**

| framing | size | ratio | vs 1-frame |
|---|---|---|---|
| one frame / bundle | 10.3 MiB | 4.52× | baseline |
| **one frame / 1,000 docs** | 11.0 MiB | 4.24× | **+6.5%** |
| one frame / doc | 17.5 MiB | 2.66× | +70.2% |
| per-doc + trained dict | 13.1 MiB | 3.56× | +27.0% |

Three findings:

- **Going 10k → 1k costs almost nothing per frame.** Lane B +0.5%, because same-site pages are
  adjacent (crawl/cluster order) so redundancy is local, and 1,000 docs (~80 MiB) already
  saturate zstd's match window — 10k mostly sits *outside* the window anyway.
- **Lane A is ~13× more sensitive (+6.5%)** — symbol dedup is its whole job, so a smaller window
  bites more — **but Lane A is small.** Topology is ~62% of the projected compressed archive
  ([0002] probe) and is **per-document → bundle-size-insensitive**; Lane B is ~34%, Lane A ~3.5%.
  Weighted: `62%×0 + 34%×0.5% + 3.5%×6.5% ≈ **+0.4% whole-archive**` (≤ ~1% even if topology
  contributed nothing).
- **Per-doc framing is expensive** (Lane B +42%, Lane A +70%) and a trained dictionary recovers
  only ~38–62% of that penalty — so per-doc compression with a shared dict is *not* a free lunch.
  It matters only if single-document random access is required; at 1k-doc read granularity,
  framing alone captures the ratio with no dictionary. (See the parked discussion below.)

## Decision

**Set `BUNDLE_CAP = 1,000.`** The whole-archive size cost is sub-1% (measured ~0.4%), the convert
peak-memory win is ~10× (the dominant term scales directly with bundle bytes), reads get a finer
random-access unit, and convert runs get finer parallelism / load-balancing. **It is trivially
reversible** — a one-line constant, pre-1.0 with no external consumers, re-pack to change — so we
take the win now rather than gate it on the unbuilt Lane A/B work.

## Consequences

- **Convert peak memory** drops ~10–14× (measured, release build, `/usr/bin/time -l`,
  `cc_000.warc.gz`):

  | config | 10k cap (before) | 1k cap (now) |
  |---|---|---|
  | 1 bundle, `THREADS=1` | ~1.1 GB | **0.07 GB** |
  | 14 cores, `in_flight_cap=28` saturated (35 bundles) | ~26 GB (projected — OOM risk) | **1.86 GB** |

  The ceiling is still `≈ in_flight_cap × per-bundle`, now ~14× smaller per bundle and bounded by
  in-flight, **not** corpus size. The full 60-segment corpus adds only its ~0.8 GB of locator +
  doc-table bookkeeping on top, so worst case ≈ **~2.7 GB** — wide headroom on a 48 GB box, where
  the 10k cap OOM-killed the machine.
- **On-disk size** grows ~+0.4% on general web (WARC) and ~+1.0% on a homogeneous ZIM once
  Lane A/B land (see the follow-up results below). Negligible today (data region empty); the
  bundle table grows ~10× (~34 KB at corpus scale — noise).
- **More runs** in convert: ~10× more, so ~10× more WARC file reopens/seeks and reorder-buffer
  churn — all negligible (total bytes decoded unchanged). ZIM bundles become ~5 clusters instead
  of ~50, still safely above the "never let bundle == cluster" floor that would replicate the
  shared dict per cluster.
- **All `BUNDLE_CAP`-referencing tests are parametric** (`BUNDLE_CAP * 2 + 3`, `div_ceil`), so
  they adapt automatically; they also run faster (smaller fixtures).

### The decoupling alternative (parked)

The same memory win is achievable at ~0% size cost by **decoupling the processing unit from the
on-disk bundle**: parse/serialize in ~1k sub-runs (memory) but seal every N into a 10k on-disk
bundle (compression window). We did not do this because the measured size cost of simply using
1,000 is sub-1% and the constant is one line. **Revisit decoupling only if a size-sensitive
per-bundle feature lands** — most plausibly **per-bundle topology compression** (the ~62%
component; same-site pages have near-identical DOM, so a large window could pay off there). If
that materializes and prefers a 10k window, decouple rather than raising `BUNDLE_CAP` back, so
convert memory and on-disk framing stay independent. Likewise, **per-document compression** (for
single-doc random access) would want the parked per-bundle trained-dictionary path, which recovers
only part of the per-doc penalty — measure before adopting.

### Follow-up results (2026-06-17)

Ran the harness (now `SPIKE_INPUT`, any source) over **3 WARC segments + a ZIM**, ~10k-doc
samples, zstd-19. The `1k vs one-frame` delta per lane:

| input | Lane B (text) | Lane A (symbols) | whole-archive est. |
|---|---|---|---|
| `cc_000` (WARC) | +0.5% | +6.5% | ~+0.4% |
| `cc_001` (WARC) | +0.7% | +6.6% | ~+0.4% |
| `cc_002` (WARC) | +0.6% | +6.7% | ~+0.4% |
| `wiktionary_co` (ZIM) | **+24.0%** | **+8.6%** | **~+1.0%** |

- **WARC is robust:** Lane B +0.6% / Lane A +6.6% across all three segments — the ~+0.4%
  whole-archive holds.
- **ZIM's lanes are far more framing-sensitive** (Lane B +24%!) because homogeneous corpora have
  long-range redundancy a 1k window can't reach. **But the whole-archive hit is still ~+1.0%**:
  topology is ~93% of the wiktionary archive (23.1 MiB vs 0.6 MiB Lane B + 1.2 MiB Lane A — tiny
  entries, big structure), so the sensitive lanes are a small share. The two effects nearly cancel.
- **Why text-heavy corpora are *not* a worse case:** per-byte framing sensitivity is highest for
  *small* documents — wiktionary entries are ~1.8 KB, so even 1,000 of them (~1.8 MB) sit below
  zstd's match window and a 10k window still captures more. Large documents (≈WARC's ~80 KB, or
  full-article ZIMs) already fill the window at 1,000 docs, so their Lane B delta is small (the
  WARC +0.6%). The worst per-byte case (tiny docs) also has proportionally large topology, capping
  the whole-archive cost.

**Conclusion: 1,000 confirmed.** Sub-1% on general web (robust), ~1% on a homogeneous ZIM.
Residual unknown: a very large, homogeneous, *text-heavy* ZIM (e.g. full Wikipedia) — its lanes
would be a bigger archive share; the window-saturation argument predicts a small delta there too,
but it is unmeasured (the 8.5 GB `wiktionary_en` was out of scope for this pass). If such a corpus
ever becomes size-critical, re-measure and prefer the decoupling path over a larger `BUNDLE_CAP`.
The harness (`SPIKE_INPUT`/`SPIKE_LANE`/`SPIKE_LEVEL`, ignored test) is kept for that and for
re-measuring once Lane B lands.
