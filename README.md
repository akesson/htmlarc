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
  whole corpus are fast.
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

## Ingesting a ZIM (`zim2htmlarc`)

The workspace also ships `zim2htmlarc`, a small companion that turns a **ZIM** file (the
Kiwix/Wikipedia offline format) into a `.htmlarc` archive — straight from a downloaded dump
to a queryable archive:

```sh
# Export every HTML article from a ZIM into one archive:
zim2htmlarc export wikipedia.zim wikipedia.htmlarc

# Only export articles whose title is in a list (one per line):
zim2htmlarc export wikipedia.zim subset.htmlarc --list words.txt

# Inspect a ZIM without converting:
zim2htmlarc list wikipedia.zim                 # "title <TAB> url", one per article
zim2htmlarc extract wikipedia.zim 'Some Title' # print one article's HTML
```

Then query the result with `htmlarc` as usual (`htmlarc probe wikipedia.htmlarc -p '…'`).

- It reads ZIM via the pure-Rust [`zim`] crate (MIT/Apache), so **no system libzim** is
  required — but building it does need a **C compiler** (`zstd-sys`/`lzma-sys` compile bundled
  C) and pulls in ~110 transitive crates. This is isolated to the `zim2htmlarc` binary; the
  three core `htmlarc-*` crates stay pure-Rust with no C dependencies.
- We depend on a **fork** of `zim` 0.4: the upstream crate is unmaintained and fails to open
  any current Kiwix dump (modern ZIMs omit the legacy title pointer list, which upstream slices
  unconditionally → `OutOfBounds`). The fork guards that sentinel.
- `extract` matches an **exact** title (the pure-Rust reader has no full-text search).
- Articles with an empty title are keyed by their URL slug.

[`zim`]: https://github.com/akesson/zim

## How it works

The DOM (`htmlarc-dom`) is **structure-of-arrays**: each node is a fixed-stride record in a
single `Vec<u8>`, with parent/sibling/child links packed as adaptive `u16` or `u24` indices
(17- or 22-byte records); text, attributes and classes are interned into side tables. Documents
are built at `u24` width and **down-packed to `u16`** on save when they fit comfortably under
the `u16` ceiling. The whole thing derives `rkyv::Archive`, so it serializes to disk and is read
back **without pointer fix-ups or per-node allocation** — traversal is array indexing into hot,
contiguous memory rather than chasing heap pointers.

An archive (`htmlarc-format`) is just a sorted `Vec` of `(key, dom)` entries written with rkyv;
`get` binary-searches by key, and `MmapArchive` reads the archived bytes **zero-copy** from a
memory-map (no deserialization).

## Honest limitations

- **~16.7M nodes per document.** Node links are packed as `u24` (the public `NodeIndex` is a
  `u32`), so the per-document ceiling is `2^24 − 1` nodes. The design still targets many *small*
  documents (dictionary entries, etc.); documents that fit are stored at half the link width
  (`u16`).
- **The parser is pragmatic, not spec-compliant.** It is a stack-based builder validated
  against real-world (messy) Wiktionary HTML — it is *not* the WHATWG tree-construction
  algorithm and will not handle every adversarial/malformed input the way `html5ever` does.

## Workspace layout

```
crates/
  htmlarc-dom/      flat, rkyv-archivable HTML DOM + parser + CSS3 selector engine
  htmlarc-macros/   `css!(...)` — compile-time-validated CSS selectors
  htmlarc-format/   the single-file .htmlarc archive (build / open / query / diff)
cli/
  htmlarc/          the `htmlarc` binary (pack / list / probe / diff)
  zim2htmlarc/      converts a ZIM (Kiwix/Wikipedia) into a .htmlarc archive
```

## Building & testing

```sh
cargo build --workspace
cargo nextest run            # tests use cargo-nextest (process-per-test)
```

Tests are run with [cargo-nextest]. Some snapshot tests use insta's `glob!` over the same
fixtures and rely on nextest's process-per-test isolation; running them with the default
`cargo test` (in-process threads) can produce spurious failures.

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
