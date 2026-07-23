# 0009 — Typed per-document metadata columns in the archive footer

- **Status:** Accepted
- **Date:** 2026-07-23
- **Scope:** `crates/htmlarc-archive` (format v12: `meta`, `trailer`, `header`, writer/reader,
  `mmap`, `builder`), `crates/htmlarc-py` (`ArchiveBuilder(meta_schema=...)`, `add(meta=...)`,
  `Document.meta`, `Archive.meta_schema`/`meta_table()`, `scan_table(meta=[...])`)
- **Companion:** extends the footer-indexed v4 container ([trailer](../../crates/htmlarc-archive/src/trailer.rs));
  orthogonal to the string lanes (0001/0005/0006/0008).

## Context

Every corpus carries per-document facts the DOM does not: source URL, fetch date, HTTP status,
byte size. The recipes proved a **sidecar parquet** works (build writes `*_meta.parquet`; every
query joins on `key`), but the pattern has real costs: two files to move together, a join in
every notebook, and no way for `Document`-level code to see its own row. Metadata is ~0.1% of an
archive's bytes, so any storage model is size- and speed-neutral at archive scale; the choice is
about *typing and access shape*. Options considered:

1. **str→str dict per doc** — simplest, but everything comes back stringly.
2. **Arbitrary JSON per doc** — flexible, but a `serde_json` parse per doc per sweep and
   string-only columns.
3. **Typed columns, schema declared per archive** — chosen: values stay typed end-to-end into
   Arrow (int64/float64/boolean/utf8), whole columns export as a memcpy, and the sidecar's
   dataframe ergonomics move *inside* the file.

## Decision

A single optional **columnar rkyv blob in the footer region**, located by a new
`meta_offset`/`meta_len` pair in the trailer (88 → 104 bytes; format version 11 → 12, exact-match
readers reject older files as before).

- **Schema:** declared once per archive — field names + one of four scalar types
  (`Str`/`Int`(i64)/`Float`(f64)/`Bool`), all nullable. `key` is reserved.
- **Layout:** parallel `names`/`types` plus one column per field. Scalars are value vectors;
  strings are concatenated bytes + cumulative u32 end offsets (4 GiB/column cap, checked at
  build). Validity is one byte per row — bit-packing would save ~0.04% of an archive.
- **Row identity:** row `i` = doc-table position `i` (arrival order). Dedup is handled where
  rows accumulate (`HtmlArchiveBuilder`), so a skipped duplicate never consumes a row; the
  writer re-validates `rows == docs` at `finish`.
- **Validation:** structure via safe rkyv access + `validate_archived` (type codes, parallel
  lengths, row counts, monotonic offsets) **eagerly at open** — the table is tiny — so per-row
  reads go unchecked.
- **Write path:** the writer takes a complete `MetaTable` (`set_meta_table`) at finish-time;
  no per-push alignment protocol. The owned `HtmlArchive` round-trips the table through
  `read_from`/`write_to`, so re-packs preserve it. The convert CLI is unchanged (no metadata
  source yet).

Python surface: `ArchiveBuilder(meta_schema={"url": str, "status": int, ...})`,
`add(key, html, meta={...})` (missing fields null, unknown fields error, `int`→`float` coerces,
`bool` never coerces to `int`), `Document.meta` → dict, `Archive.meta_schema` → dict,
`Archive.meta_table()` → Arrow table (the sidecar replacement), and `scan_table(meta=[...])`
appending typed columns — each match row carries its document's value, eliminating the join.

## Consequences

- One file instead of two; `scan_table` results need no post-hoc join; typed columns survive
  into polars/pyarrow/duckdb without casts.
- Format bump: v11 archives must be re-packed (pre-1.0, no compatibility promise).
- The schema is fixed at build time; adding a field later means re-packing (append — ADR 0010
  territory — must extend rows with the *same* schema).
- Per-bundle metadata was rejected: no query touches metadata bundle-locally, and a footer blob
  keeps `BundleDesc`/contiguity validation untouched.
