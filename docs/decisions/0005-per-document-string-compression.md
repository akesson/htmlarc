# 0005 — Per-document string compression with one archive-wide dictionary

- **Status:** Accepted
- **Date:** 2026-06-18
- **Scope:** `crates/htmlarc-archive` (format v10, `codec`, `bundle_strings`, writer/reader),
  `crates/htmlarc-dom` (`StringSource`/`FrameDecoder` seam), `cli/htmlarc-convert` (two-phase
  convert)
- **Companion:** compresses the Lane B text block that [0001](0001-string-storage-lanes.md)
  reserved and [0009 format v9] relocated into the per-bundle
  [`BundleStrings`](../../crates/htmlarc-archive/src/bundle_strings.rs) region; sized against the
  bundle of [0004](0004-bundle-size-1000.md).

## Context

Through v9 the per-bundle `BundleStrings` block — every document's relocated text/comment pool —
was stored **uncompressed**: a read was a zero-copy mmap slice (`StringSource::Plain`). On real
web text (`cc_000.warc.gz`) that block is ~65% of the archive. The seam for compressing it
(`StringSource::Lazy` / `LazyState`) already existed but was dormant.

The open questions were the *granularity* (how many documents share a compressed frame), whether a
trained **dictionary** is worth it, and at what cost to random vs sequential read. A committed
evaluation harness (`benches/string_access.rs` for the read pattern; the `framing` subcommand for
size/decode over the real on-disk bytes) answered them on `cc_000` and a multilingual mix:

- **Granularity:** per-document frames (one frame = one document). Larger slices barely improve the
  ratio on web text (cc0 L3: a slice of 1 vs 1,000 docs moved only 4.76× → ~7×) but make a random
  single-doc read inflate the whole slice (cold-get 30 µs → 23 ms at S=1000). Per-document keeps
  random ≈ sequential.
- **Dictionary:** one **archive-wide** ("global") dictionary, not per-bundle. A global dict matches
  the per-bundle dict's ratio at ~5× the build speed (one training, not one per bundle), and is a
  safe floor on multilingual data (never worse than dictionary-less on a minority script).
- **Level:** zstd **L3**. Decoding is level-independent, so the level is a build-time-only knob —
  raisable later with no read-path or format change.
- **Sample size:** the ratio plateaus by ~500 documents; training on the first ~1–2 bundles is
  enough (prefix ≈ strided sampling within ~1%).

## Decision

Format **v10**: each document's relocated pool is one independent zstd-L3 frame, optionally against
one archive-wide dictionary.

- **Block** (`bundle_strings.rs`): `BundleStrings` stores concatenated per-document frames plus two
  cumulative offset tables — `frame_offsets` (locating each frame) and `raw_offsets` (each frame's
  exact decompressed length, used to size the inflate buffer and to validate). A text-free document
  is a zero-length frame and skips the codec on both sides.
- **Dictionary storage** (`trailer.rs`): the vestigial "data region" trailer slot is repurposed as
  the dictionary region (`dict_offset`/`dict_len`); `dict_len == 0` means dictionary-less. No
  trailer-layout change. Capped at 110 KiB (`DICT_MAX`).
- **Read seam** (`string_source.rs`): `LazyState`'s bare `decode: fn` became `decoder: &dyn
  FrameDecoder` — a codec-agnostic trait so `htmlarc-dom` never depends on zstd; `htmlarc-archive`
  installs a `ZstdFrameDecoder` holding the prepared `DDict` (built once at open). Inflation stays
  **lazy and per-document**: a query that never reads a document's text never decompresses it (a
  pure selector sweep pays nothing — the canary). The inflate caches (`OnceCell`s) live in the
  caller's read scope, so `MmapArchive` owns no per-read state and stays immutable and `Sync`.
- **Write path** is **two-phase, worker-side** (`cli/htmlarc-convert/convert.rs`): a single-threaded
  warm-up parses the first runs (~2,000 docs) and trains the dictionary; the remaining runs stream
  in parallel, each worker compressing its run's text against the shared, immutable `CDict` ("a
  bundle per core") before the coordinator writes it. The warm-up runs (parsed before the dict
  existed) are compressed once on the coordinator. The in-memory builder path compresses
  dictionary-less through the same single storage path.

No back-compat shim — v9 and older must be re-packed (consistent with the pre-1.0 clean-slate
stance).

## Measurement (shipped format, `cc_000.warc.gz`, 20,000 docs)

| | string block | ratio | whole archive |
|---|---|---|---|
| v9 (uncompressed) | 1.30 GiB | 1.00× | ~2.6 GB |
| v10 dictionary-less | 278 MiB | 4.76× | 1.479 GB |
| **v10 + global dict (110 KiB)** | **252 MiB** | **~5.25×** | **1.453 GB** |

Read pattern (`string_access` bench, full-text sweep of a multi-bundle archive):

| | sequential | random |
|---|---|---|
| v9 (zero-copy `Plain`) | 580.6 ms | 581.8 ms |
| v10 (per-doc frame, lazy inflate) | 672.4 ms | 671.2 ms |

Random ≈ sequential holds (no slice amplification); a full-text sweep costs ~16% more, paid only
when text is actually read. The dictionary adds ~+10% ratio for a one-time, sub-second training
cost, stored once (110 KiB) per archive.

## Consequences

- Text reads now inflate (lazily, cached per document); topology/selector queries that touch no
  text are unaffected. A document bound via `MmapArchive::doc(i)` is the new owning `Doc` handle
  (implements `DomRead`/`DomRef`), transparent at call sites through the trait.
- `convert` is two-phase: a brief single-threaded warm-up (train) precedes the parallel stream. The
  warm-up buffers only compact serialized entries (~2k docs), so peak memory stays bounded
  ([0004](0004-bundle-size-1000.md)).
- Level (L3) and dictionary size (110 KiB) are the remaining knobs; both are read-compatible to
  change at build time. Raising the level only slows convert, never reads.
- The `framing` subcommand and `framing_spike` harness that chose these constants are now
  superseded by the shipped format (they read the block back by inflating it).
