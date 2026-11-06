# 0011 — Charset decoding at ingest

- **Status:** Accepted
- **Date:** 2026-11-01
- **Scope:** `cli/htmlarc-convert` (`source/decode.rs`, WARC and directory sources)

## Context

The parser takes `&str`, and the WARC and directory sources produced it with
`String::from_utf8_lossy`. Every page in a legacy charset lost its non-ASCII text for good:

```text
<title>\xcf\xf0\xe8\xe2\xe5\xf2</title>, charset=windows-1251
before: <title>������</title>
after:  <title>Привет</title>
```

On `cc_000` that was 1,068 of 34,465 HTML pages (3.1%) and 2.34M replacement characters, with no
warning. A browser's order (BOM, then header, then `<meta>`, then detection) doesn't fit crawl
data as it stands, because labels are wrong often enough to matter there: 12 pages have a
non-UTF-8 header on valid non-ASCII UTF-8, and 3 more are UTF-8 with a few bad bytes under a
legacy label, up to 18,484 valid characters. Believing the label would garble those whole pages.

## Decision

Decode each page once, in the source, in this order:

1. **BOM:** use its encoding and remove it.
2. **Valid UTF-8:** use the bytes as they are, whatever the labels say. Nearly every page stops
   here, so conversion keeps the old fast path.
3. **Damaged UTF-8:** keep UTF-8, replacing the bad bytes, when a page has at least 4 valid
   multi-byte characters per invalid sequence. An incomplete sequence at the very end doesn't
   count as invalid, because that is what truncating a record leaves. On `cc_000`, pages with a
   legacy label were below 4 or above 64, with none between.
4. **Label:** the `Content-Type` charset, then `<meta>` in the first 4,096 bytes. A UTF-8 label
   is skipped, because the bytes have just disproved it. `<meta>` can't claim UTF-16, and
   replacement-only labels such as `iso-2022-kr` are ignored, because they would blank the page.
5. **Detection:** `chardetng`, with the URL's top-level domain as a hint. The hint raised
   agreement with declared labels from 91% to 93%.

The header stays ahead of `<meta>`: they disagreed on 7 legacy pages, and each pair was nearly
the same encoding, such as ISO-8859-15 and windows-1252.

## Consequences

- **Result on `cc_000`:** replacement characters fell from 2,336,553 to 20,391. Of those left,
  10,784 are in one PDF served as `text/html` and 9,337 are on 247 pages whose output didn't
  change. One page rose from 115 to 119, in a garbled script comment.
- **Valid UTF-8 output is unchanged,** except that 371 pages lost a leading BOM, which used to
  reach the DOM as text. 33,134 of 34,464 pages are byte-identical.
- **Conversion time** didn't change measurably on `cc_000` (about 12 s either way).
- **Existing archives keep their damage.** The lost bytes aren't stored, so fixing a page means
  rebuilding from the source. The file format doesn't change.
- **Stored `<meta charset>` keeps its original value** even though text is now UTF-8, so
  re-serialized HTML can name a charset it isn't in. It is what the source said, and extraction
  doesn't use it.
- **ISO-2022-JP pages aren't decoded.** They are 7-bit, so they pass as valid UTF-8 and keep their
  escape sequences. `cc_000` has none, so this isn't handled.
- **Decoding is silent.** The converter reports no count of pages decoded or guessed.
- **Not covered:** ZIM content is UTF-8 by format, and `ArchiveBuilder.add` takes a decoded `str`,
  so charsets are the caller's concern there.
