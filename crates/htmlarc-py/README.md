# htmlarc

Python bindings for [htmlarc](https://github.com/akesson/htmlarc): query pre-parsed HTML
document archives (`.htmlarc`) with CSS selectors.

An `.htmlarc` file stores documents as ready-to-query DOMs, so opening an archive of
millions of pages and selecting into any of them requires **no HTML parsing at read
time** — the archive is memory-mapped and documents resolve lazily.

```python
import htmlarc

# Parse a single document (fault-tolerant, html5-style recovery)
doc = htmlarc.parse("<html><body><h1 class='title'>Hi</h1></body></html>")
for el in doc.select("h1.title"):
    print(el.text, el.css_path)

# Build an archive from HTML strings
builder = htmlarc.ArchiveBuilder()
builder.add("page-1", "<html>...</html>")
builder.write("corpus.htmlarc")

# Query it — documents come back pre-parsed
archive = htmlarc.open("corpus.htmlarc")
title = archive["page-1"].select_first("h1")

# Compile a selector once when scanning many documents
h1 = htmlarc.Selector("h1.title")
for doc in archive:
    for el in doc.select(h1):
        print(doc.key, el.text)
```

## Building from source

Requires Rust and [maturin](https://github.com/PyO3/maturin):

```sh
maturin build --release -m crates/htmlarc-py/Cargo.toml
```

Tests (from the repository root, any Python ≥ 3.10 environment with the wheel installed):

```sh
pytest crates/htmlarc-py/tests
```
