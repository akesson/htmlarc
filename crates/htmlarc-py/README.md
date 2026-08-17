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

# When the question is "how many", count in Rust — no strings cross into Python and
# text blocks are never decompressed, so this is the fastest sweep of all.
n_links = archive.scan_count("a[href]", attr="href")   # one int, all cores, GIL released
n_heads = archive.scan_count("h1, h2, h3")

# Feeding a dataframe? scan_table returns the whole sweep as one flat Arrow table — one row
# per matched element, a `key` column (the document key), an optional `text` column, and one
# nullable column per requested attribute (`"class"` synthesized like `.get()`). It builds the
# columns off-GIL and hands them over zero-copy via the Arrow PyCapsule interface, so nothing is
# marshalled per match — much faster than scan_text/scan_attr when extracting from everything.
# htmlarc has no Arrow dependency itself; the consumer (pyarrow/polars/duckdb) provides it.
import polars, pyarrow
links = pyarrow.table(archive.scan_table("a[href]", attrs=["href"]))   # columns: key, href
heads = polars.DataFrame(archive.scan_table("h1, h2, h3", text=True))  # columns: key, text
# Text and attributes in a single sweep (two scan_* calls would parse-walk twice):
df = polars.DataFrame(archive.scan_table("a[href]", text=True, attrs=["href"]))

# Batch extraction on a single document avoids per-element FFI calls too:
doc.select_text("p")            # list[str]
doc.select_attr("a", "href")    # list[str | None]
doc.select_count("a", attr="href")  # int — count matches (optionally attribute-bearing)
doc.select_html(".figure")      # list[str]

# Asking the same documents several questions? Hold the handles. Each document
# handle caches the text blocks it has decompressed, but the cache lives on the
# handle — `archive[key]` and iteration hand out fresh ones. Reusing handles makes
# every text read after the first hit warm caches (~2x on text-heavy sweeps).
docs = list(archive)
titles = [d.select_text("h1") for d in docs]   # decompresses matched text blocks
paras = [d.select_text("p") for d in docs]     # reuses them where they overlap

# matching() also takes a Filter for include/exclude rules; a pure key filter
# never touches document bodies at all.
f = htmlarc.Filter(include_css="article .main", exclude_keys=["page-1"])
keys = archive.matching(f)
```

The module ships type stubs (`htmlarc.pyi`), so editors and type checkers see the
full API.

## License

htmlarc is **dual-licensed**: [AGPL-3.0](https://github.com/akesson/htmlarc/blob/main/LICENSE)
or a paid commercial license. The wheel statically links the Rust core, so the whole
package — bindings included — is AGPL.

What that means in practice:

- **Scripts, research, internal analysis, anything you'd release under the AGPL** — free,
  no strings beyond the AGPL's own.
- **Shipping htmlarc inside a closed-source product, or serving its functionality to users
  over a network (SaaS)** — the AGPL requires releasing your source; the
  [commercial license](https://github.com/akesson/htmlarc/blob/main/COMMERCIAL.md) removes
  that obligation. Pricing is public in that document.
- **Organization bans AGPL dependencies?** Same answer — the commercial license is the
  supported path.

## Building from source

Requires Rust and [maturin](https://github.com/PyO3/maturin):

```sh
maturin build --release -m crates/htmlarc-py/Cargo.toml
```

Tests (from the repository root, any Python ≥ 3.10 environment with the wheel installed):

```sh
pytest crates/htmlarc-py/tests
```
