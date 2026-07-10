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

# Or let the archive sweep itself: matching()/scan_text()/scan_attr() fan out
# across all cores with the GIL released — much faster than the Python loop above.
for key, texts in archive.scan_text(h1):
    print(key, texts)
links = archive.scan_attr("a[href]", "href")

# Batch extraction on a single document avoids per-element FFI calls too:
doc.select_text("p")            # list[str]
doc.select_attr("a", "href")    # list[str | None]
doc.select_html(".figure")      # list[str]

# matching() also takes a Filter for include/exclude rules; a pure key filter
# never touches document bodies at all.
f = htmlarc.Filter(include_css="article .main", exclude_keys=["page-1"])
keys = archive.matching(f)
```

The module ships type stubs (`htmlarc.pyi`), so editors and type checkers see the
full API.

## Building from source

Requires Rust and [maturin](https://github.com/PyO3/maturin):

```sh
maturin build --release -m crates/htmlarc-py/Cargo.toml
```

Tests (from the repository root, any Python ≥ 3.10 environment with the wheel installed):

```sh
pytest crates/htmlarc-py/tests
```
