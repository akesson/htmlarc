# htmlarc

A toolkit for analysing **large corpora of HTML** efficiently. It stores many pre-parsed
HTML documents in one file (`.htmlarc`) and lets you CSS-query them **without re-parsing** —
useful when you have gigabytes of HTML (e.g. a Wiktionary dump) split across hundreds of
thousands of small files.

> [!NOTE]
> **Stable, but not under active development.** Dual-licensed: **[AGPL-3.0](LICENSE)** for
> open-source use, or a **[commercial license](COMMERCIAL.md)** to use it in closed-source or
> SaaS software without AGPL obligations. There is no free support or roadmap, but **I'm
> available for paid contract work** to extend, integrate, or maintain it for your use case —
> reach me at [@akesson](https://github.com/akesson).

## Why

- **No filesystem overhead.** Hundreds of thousands of tiny `.html` files waste a huge amount
  of space and inodes, and are slow to traverse. One archive avoids all of that.
- **No HTML parsing at query time.** Documents are parsed once into a compact, flat binary DOM
  and stored with [rkyv]. A packed `.htmlarc` is queried **zero-copy** straight from a
  memory-map — no HTML parse, and no per-node deserialization — so repeated CSS queries over the
  whole corpus are fast. (Per-document **text** payloads are zstd-compressed and inflate lazily on
  read; topology and selector matching stay zero-copy, so a pure selector sweep never decompresses.)
- **A real CSS3 selector engine** runs over that DOM (compound/complex/relative selectors,
  `:has()`, `nth-*`, attribute operators), at speeds comparable to a pointer-tree DOM.

## Quick start

```sh
cargo build --release

# Every command takes a <source>, which is one of:
#   • a .htmlarc archive file   (loaded directly, no parsing)
#   • a single .html/.htm file  (parsed into a one-entry archive)
#   • a directory of *.html     (each parsed, keyed by file name)

# Pack a folder of HTML into one archive (parse once):
htmlarc pack ./pages -o pages.htmlarc

# List the documents (keys), optionally filtered:
htmlarc list pages.htmlarc
htmlarc list pages.htmlarc -i 'css: section > h1'
htmlarc list page.html                     # a single loose file works too

# Count CSS-selector matches across the whole corpus:
htmlarc probe pages.htmlarc -p 'section > h1 => HtmlFmt[id][class^=mw]@words'

# Diff two sources by per-document checksum (fast):
htmlarc diff old.htmlarc new.htmlarc
```

Run `htmlarc --help` (or `htmlarc <command> --help`) for the full options.

## Ingesting documents (`htmlarc-convert`)

The workspace also ships `htmlarc-convert`, a companion that turns a **ZIM** file (the
Kiwix/Wikipedia offline format), a **WARC** web-crawl file (e.g. Common Crawl, plain or
`.warc.gz`), or a **directory** of saved HTML files into a `.htmlarc` archive. The format is
inferred from the input path (override with `--format zim|warc|dir`):

```sh
# Convert every HTML document from a source into one archive:
htmlarc-convert convert wikipedia.zim wikipedia.htmlarc
htmlarc-convert convert crawl.warc.gz crawl.htmlarc
htmlarc-convert convert ./saved-pages/ pages.htmlarc

# Only convert documents whose key is in a list (one per line):
htmlarc-convert convert wikipedia.zim subset.htmlarc --list words.txt

# Sample a huge input:
htmlarc-convert convert crawl.warc.gz sample.htmlarc --limit 50000

# Inspect a source without converting:
htmlarc-convert list wikipedia.zim                 # one document key per line
htmlarc-convert extract wikipedia.zim 'Some Title' # print one document's HTML
```

Then query the result with `htmlarc` as usual (`htmlarc probe wikipedia.htmlarc -p '…'`).

- ZIM is read via the pure-Rust [`zim`] crate (MIT/Apache), so **no system libzim** is
  required — but building it does need a **C compiler** (`zstd-sys`/`lzma-sys` compile bundled
  C) and pulls in ~110 transitive crates. This is isolated to the `htmlarc-convert` binary; the
  three core `htmlarc-*` crates stay pure-Rust with no C dependencies.
- We depend on a **fork** of `zim` 0.4: the upstream crate is unmaintained and fails to open
  any current Kiwix dump (modern ZIMs omit the legacy title pointer list, which upstream slices
  unconditionally → `OutOfBounds`). The fork guards that sentinel.
- Document keys are the ZIM title (URL slug if the title is empty), the WARC `WARC-Target-URI`,
  or the file path relative to the directory root. `extract` matches a key **exactly**.
- The WARC reader currently loads selected documents into memory; for very large crawls use
  `--limit`. (Parser error-recovery has since landed — see *Honest limitations* — so converting
  general web HTML now drops only ~0.0005% of documents, all genuine capacity overflow rather than
  malformed markup.)

[`zim`]: https://github.com/akesson/zim

## How it works

The DOM (`htmlarc-dom`) is **structure-of-arrays**: each node is a fixed-stride record in a
single `Vec<u8>`, with parent/sibling/child links packed as adaptive `u16` or `u24` indices
(17- or 22-byte records); text, attributes and classes are interned into side tables. Documents
are built at `u24` width and **down-packed to `u16`** on save when they fit comfortably under
the `u16` ceiling. The whole thing derives `rkyv::Archive`, so it serializes to disk and is read
back **without pointer fix-ups or per-node allocation** — traversal is array indexing into hot,
contiguous memory rather than chasing heap pointers.

An archive (`htmlarc-archive`) is a **bundle-segmented, footer-indexed** container: documents are
grouped into bundles (up to 1,000) and written with rkyv, with a sorted key index in the footer.
`get` binary-searches that index by key, and `MmapArchive` reads the archived topology **zero-copy**
from a memory-map. Each document's text/comment payload is relocated into a per-bundle block and
stored as an independent zstd frame, inflated lazily per document on first text read — so
selector/topology queries, which touch none of it, stay fully zero-copy.

## Honest limitations

- **~16.7M nodes per document.** Node links are packed as `u24` (the public `NodeIndex` is a
  `u32`), so the per-document ceiling is `2^24 − 1` nodes. The design still targets many *small*
  documents (dictionary entries, etc.); documents that fit are stored at half the link width
  (`u16`).
- **Spec-compliant tokenizer, pragmatic tree builder.** Tokenization uses [html5gum] (a WHATWG
  spec-compliant tokenizer); the tree builder is an in-house stack-based builder with HTML5 **error
  recovery** (stray/mismatched end tags, foreign SVG/MathML, optional tags) rather than the full
  WHATWG tree-construction algorithm (no adoption-agency / foster-parenting). On a 2.04M-document
  Common Crawl corpus it parses **99.9995%** of `text/html` documents; the ~0.0005% dropped are
  genuine capacity overflow (the node ceiling above), not malformed markup.

## Workspace layout

```
crates/
  htmlarc-dom/      flat, rkyv-archivable HTML DOM + parser + CSS3 selector engine
  htmlarc-archive/  the single-file .htmlarc archive (build / open / query / diff)
cli/
  htmlarc/          the `htmlarc` binary (pack / list / probe / diff)
  htmlarc-convert/  converts ZIM / WARC / a directory of HTML into a .htmlarc archive
```

## Building & testing

```sh
cargo build --workspace
cargo nextest run            # tests use cargo-nextest (process-per-test)
```

Tests are run with [cargo-nextest]. Some snapshot tests use insta's `glob!` over the same
fixtures and rely on nextest's process-per-test isolation; running them with the default
`cargo test` (in-process threads) can produce spurious failures.

### Git hooks (one-time setup)

This repo ships git hooks in `.githooks/` that mirror the CI checks: `pre-commit` runs
`cargo fmt --check` + clippy, and `pre-push` additionally runs the test suite (nextest).
Git does **not** pick them up automatically — activate them once per clone with:

```sh
make setup
```

The other `make` targets mirror the CI jobs so you can reproduce a failure locally:
`make fmt`, `make lint`, `make test`, `make bench`, or `make ci` for all of them. Run
`make` to list them.

## Example

A runnable, end-to-end example of using htmlarc as a library — the same **step-wise reduction**
that powers a Wiktionary pipeline (raw MediaWiki page → cleaned `<head>` → article content only),
with a `.htmlarc` archive as the checkpoint between each pass:

```sh
cargo run -p htmlarc-archive --example wiktionary
```

It parses real Swedish Wiktionary pages, mutates each DOM (strip chrome, lift the article body),
re-packs each stage into its own archive, then reopens the result zero-copy and CSS-queries the
definitions out — printing the size shrink at every step. See
[`crates/htmlarc-archive/examples/wiktionary.rs`](crates/htmlarc-archive/examples/wiktionary.rs).

## License

Dual-licensed:

- **[GNU AGPL-3.0](LICENSE)** — free for open-source use. If you distribute htmlarc (or a work
  based on it), or offer it to users over a network, the AGPL requires you to make the
  corresponding source available under the AGPL.
- **[Commercial license](COMMERCIAL.md)** — to use htmlarc in closed-source or SaaS software
  without AGPL obligations. Contact [@akesson](https://github.com/akesson).

Test/benchmark fixtures under `crates/htmlarc-dom/src/**` and `cli/htmlarc/src/testdata/` are
real Wikimedia (Wiktionary) pages, licensed CC BY-SA — see [NOTICE](NOTICE).

[rkyv]: https://rkyv.org
[cargo-nextest]: https://nexte.st
[html5gum]: https://github.com/untitaker/html5gum
