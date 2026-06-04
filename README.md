# htmlarc

A toolkit for analysing **large corpora of HTML** efficiently. It stores many pre-parsed
HTML documents in one file (`.htmlarc`) and lets you CSS-query them **without re-parsing** —
useful when you have gigabytes of HTML (e.g. a Wiktionary dump) split across hundreds of
thousands of small files.

> [!IMPORTANT]
> **Unmaintained.** This is an extraction from an abandoned project, published in the hope it
> is useful. Use it if you like — there is no support, no roadmap, and pull requests may not
> be reviewed. The interesting parts are the ideas; the code is provided as-is under
> MIT/Apache-2.0.

## Why

- **No filesystem overhead.** Hundreds of thousands of tiny `.html` files waste a huge amount
  of space and inodes, and are slow to traverse. One archive avoids all of that.
- **No HTML parsing at query time.** Documents are parsed once into a compact, flat binary DOM
  and stored with [rkyv]. Loading an archive is a cheap deserialize of contiguous buffers, not
  an HTML parse — so repeated CSS queries over the whole corpus are fast.
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

The DOM (`htmlarc-dom`) is **structure-of-arrays**: each node is a fixed 17-byte record in a
single `Vec<u8>`, with `u16` parent/sibling/child indices; text, attributes and classes are
interned into side tables. The whole thing derives `rkyv::Archive`, so it serializes to disk
and loads back without pointer fix-ups or per-node allocation. Traversal is array indexing
into hot, contiguous memory rather than chasing heap pointers.

An archive (`htmlarc-format`) is just a sorted `Vec` of `(key, dom)` entries written with rkyv;
`get` binary-searches by key.

## Honest limitations

- **Max 65,535 nodes per document.** Node indices are `u16` — deliberate, since the design
  targets many *small* documents (dictionary entries, etc.). Large pages will overflow this.
- **The parser is pragmatic, not spec-compliant.** It is a stack-based builder validated
  against real-world (messy) Wiktionary HTML — it is *not* the WHATWG tree-construction
  algorithm and will not handle every adversarial/malformed input the way `html5ever` does.
- **It deserializes, it does not mmap-in-place.** "No HTML parsing" is accurate — loading an
  archive is a cheap rkyv deserialize of flat buffers. It is *not* zero-copy `mmap` querying
  of the archived bytes (the SoA layout would make that a small change, but it isn't done).

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

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.

Test/benchmark fixtures under `crates/htmlarc-dom/src/**` and `cli/htmlarc/src/testdata/` are
real Wikimedia (Wiktionary) pages, licensed CC BY-SA — see [NOTICE](NOTICE).

[rkyv]: https://rkyv.org
[cargo-nextest]: https://nexte.st
