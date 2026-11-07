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
   multi-byte characters per invalid sequence. On `cc_000`, pages with a legacy label were below
   4 or above 64, with none between. When the header or any `<meta>` says UTF-8, 1 per invalid
   sequence is enough, because the label and the valid characters agree:
   `“Hello” it’s don\x92t` stays UTF-8 with one U+FFFD instead of going to detection and turning
   every curly quote into `â€œ`. A character cut off at the very end doesn't count as invalid,
   because that is what truncating a record leaves; a stray byte there still does.
4. **Label:** the `Content-Type` charset, then each `<meta>` in the first 4,096 bytes in document
   order, and the first that isn't UTF-8 wins. UTF-8 labels are skipped because the bytes have
   just disproved them, so `<meta charset=utf-8>` followed by a real `windows-1251` label uses
   the second. Three kinds of label are refused or remapped, in the header and `<meta>` alike:
   - UTF-16 labels are refused. On a page that has already failed as UTF-8, `charset=utf-16` is
     far likelier to be wrong about an 8-bit page than right about a UTF-16 page without a BOM,
     and believing it turns every tag into CJK-looking text.
   - `x-user-defined` means windows-1252, as in a browser's prescan; its own decoder maps every
     high byte to the Private Use Area.
   - Replacement-only labels such as `iso-2022-kr` are ignored, because they would blank the page.
5. **Detection:** `chardetng`, with the URL's top-level domain as a hint. The hint raised
   agreement with declared labels from 91% to 93%.

The header stays ahead of `<meta>`: they disagreed on 7 legacy pages, and each pair was nearly
the same encoding, such as ISO-8859-15 and windows-1252.

## Consequences

- **Result on `cc_000`:** replacement characters fell from 2,336,553 to 20,391. Of those left,
  10,784 are in one PDF served as `text/html` and 9,337 are on 247 pages whose output didn't
  change. One page rose from 115 to 119, in a garbled script comment.
- **The UTF-8-label rule in step 3** changed 9 pages on `cc_000`, all with a UTF-8 label and
  mostly UTF-8 bytes mixed with some legacy ones, so neither choice decodes all of a page. On 8,
  914 characters of UTF-8 text no longer turn into mojibake (`â€œcrisisâ€�`, `Strona gÅ‚Ã³wna`,
  a Chinese title), and 538 legacy characters become U+FFFD instead. The 9th is the script-comment
  page, back to 115. Replacement characters rise by 534 in total. The legacy order, step 4's
  remapping and the cut-off-tail rule changed no page on `cc_000`.
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

## Alternatives considered

Every tool checked reads the BOM first, decodes legacy charsets by their WHATWG labels, and ends
with a statistical guess. They split on whether a label or the bytes win:

| Tool (version checked) | Order |
|---|---|
| WHATWG HTML (browsers) | BOM, header, `<meta>` in the first 1,024 bytes, optional guess, locale default (windows-1252 in most locales). `<meta>` UTF-16 counts as UTF-8 and `x-user-defined` as windows-1252. |
| Firefox | The spec, guessing with `chardetng`, which documents that browsers "must not" allow UTF-8 as a guess for web content. |
| Scrapy w3lib (`master`, 2.6.0) | BOM, header, `<meta>` or XML declaration, valid UTF-8, guess with the top-level domain, delegated to `frostwork.encoding`. Older releases map latin-1 to cp1252 and skip the UTF-8 check. |
| frostwork 0.2 (Scrapy, Rust) | BOM, BOM-less UTF-16 from `<?`, header, `<meta>` or XML declaration anywhere in the document, valid UTF-8, `chardetng` with the top-level domain. |
| Go `x/net/html/charset` | First 1,024 bytes only: BOM, header, `<meta>`, valid UTF-8, windows-1252. No guessing. |
| BeautifulSoup 4.15 | Caller's encodings, BOM, `<meta>` or XML declaration, chardet, UTF-8, windows-1252; the first that decodes without error wins. |
| requests 2.34 | Header only; `text/*` without a charset is ISO-8859-1. |
| trafilatura 2.1 | Valid UTF-8, then cchardet and charset_normalizer. No header or `<meta>`. |

- **Labels first, as browsers and Scrapy do.** Rejected. A browser has to show what authors
  tested in browsers; htmlarc extracts text, and legacy text almost never forms valid UTF-8, so
  valid UTF-8 is the stronger evidence. The cost: on the 12 `cc_000` pages with a legacy label on
  valid UTF-8, htmlarc's text differs from what bs4, lxml or Scrapy give, deliberately.
- **Bytes only, as trafilatura does.** Rejected. Without labels, every legacy page is a guess;
  `chardetng` agreed with the declared label on only 93% of `cc_000`'s legacy pages.
- **Repair mixed pages byte by byte**, like BeautifulSoup's `detwingle()` (windows-1252 inside
  UTF-8). Not done. It would beat step 3 on the 9 mixed pages, where either encoding loses some
  characters, but it handles one pair of encodings and those pages are 0.03% of `cc_000`.
- **Read XML declarations and scan for `<meta>` past 4,096 bytes**, as browsers, w3lib and
  frostwork do. Not done; measured instead. Of the 1,068 `cc_000` pages that aren't UTF-8, 3
  have only an XML declaration, all saying UTF-8, so the result wouldn't change. 7 have their only
  `<meta>` charset past 4,096 bytes, and 2 of those name a legacy charset; they go to `chardetng`.
