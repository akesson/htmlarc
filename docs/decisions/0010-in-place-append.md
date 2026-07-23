# 0010 — In-place append with a header-staged recovery offset

- **Status:** Accepted
- **Date:** 2026-07-23
- **Scope:** `crates/htmlarc-archive` (`writer` append mode, `append`/`ArchiveAppender`,
  `trailer::read_at` + recovery fallback, `header` bytes `10..16`),
  `crates/htmlarc-py` (`htmlarc.append(path)` returning the standard `ArchiveBuilder`)
- **Companion:** extends the footer-indexed container of [0009](0009-typed-metadata-columns.md)
  (same format version 12 — append changes no on-disk structure, only how a file may grow).

## Context

Adding documents to an existing archive previously meant a full load-and-rewrite
(`HtmlArchive::pack_to`), which re-parses nothing but re-serializes and re-compresses
everything — unacceptable for incremental corpora (a crawler adding a day's pages to a
multi-GB archive). The container is footer-indexed: all locating state (doc table, bundle
table, sort index, dict, metadata) lives at the tail, and bundles are self-contained — so
*appending bundles and rewriting the footer* is mechanically natural. The hard part is
crash safety: a naive footer overwrite leaves an unreadable file if the process dies
mid-append.

## Decision

**Append after the old EOF; never overwrite anything.** New bundles stream in after the
existing footer (which stays behind as dead bytes), and `finish` writes a fresh footer —
dict, extended doc/bundle tables, re-sorted key index, extended metadata table, trailer —
at the new tail.

**Crash safety = one u48 in the header.** Before the first byte is written, the old
trailer's offset is staged into the header's reserved bytes `10..16` and synced. Every
read path's trailer lookup (`Trailer::read_from_tail`) falls back to this offset when the
tail is not a valid trailer. Commit order in `finish`: new tail durable (`sync_data`) →
header cleared → synced. Consequences:

- A crashed or in-progress append leaves the file **readable as the pre-append archive**.
- The next append heals an abandoned tail (it truncates to just past the recovered trailer
  and overwrites the garbage).
- Concurrent *readers* that already mapped the file are unaffected (append only writes
  past their mapped EOF plus the header bytes, which they never re-read). Concurrent
  *appenders* are not supported.

**Dedup and metadata stay aligned.** The writer is pre-seeded with all existing keys
(first wins, old document wins); `push` reports whether a document was stored, and
`ArchiveAppender` appends a metadata row only for stored documents
(`MetaSchema::validate_row` runs *before* the document hits the file, so a bad row cannot
strand a stored document without a row). New text is compressed against the archive's
existing dictionary, so old and new frames share one decoder.

**Python:** `htmlarc.append(path)` returns the ordinary `ArchiveBuilder` (append-backed):
same `add`/`add_document`/`meta`/`on_error="skip"`/context-manager surface; `write()`
commits; leaving the `with` block on an exception abandons the append and the file stays
as it was.

## Consequences

- Appending N documents costs O(N + footer), not O(archive). Memory stays flat (streaming
  writer).
- Each committed append leaves one dead footer behind (tables + trailer + dict copy —
  typically KBs to ~100 KB against the archive's dictionary). Re-pack (`pack_to`) reclaims
  them; metadata and bundle boundaries survive the re-pack.
- The archive-wide zstd dictionary is frozen at first build: appended text compresses
  against it even if the new corpus differs. Acceptable — the dictionary is an economy,
  not a correctness device; re-pack (via convert's two-phase path) can retrain.
- A schema cannot be *introduced* by append; a metadata-less archive stays metadata-less
  until re-packed with a schema.
- True footer-overwrite append (no dead bytes) was rejected: it saves KBs at the price of
  an unreadable file on crash and a corrupted mapping for concurrent readers.
