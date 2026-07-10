# Querying HTML corpora from Python: BeautifulSoup vs lxml vs htmlarc

Head-to-head measurement of the two standard Python HTML-querying stacks against the
`htmlarc` Python bindings, on two real corpora. Basis for an article; all numbers
reproduced by the scripts next to this file (`extract.py`, `bench.py`).

## Contenders

| | Parser | Selector engine | Usage measured |
|---|---|---|---|
| **BeautifulSoup 4.15** | lxml backend (its fastest) | soupsieve 2.8.4 | `soup.select(css)` |
| **lxml 6.1.1** | libxml2 HTML parser | cssselect 1.4 (CSS→XPath) | pre-compiled `CSSSelector` |
| **htmlarc 0.1.0** | Rust (html5gum + recovering tree builder) | native CSS3 engine | pre-compiled `Selector`, `select_text`/`select_attr` batch calls, `scan_*` sweeps |

Both Python incumbents were measured in their *best-practice fast* configuration
(bs4 with the lxml backend, lxml with compiled selectors and **bytes input**), not
their defaults.

## Setup

- Apple M4 Pro (10P+4E), 48 GB RAM, macOS; CPython 3.12.13.
- **wikt**: 9,567 docs, 52.3 MB — Corsican Wiktionary (many ~5 KB pages; htmlarc's target shape).
- **cc**: 5,000 docs, 404.9 MB — a Common Crawl segment (general web HTML, avg ~81 KB, messy).
- Original HTML extracted from the source ZIM/WARC (via libzim/warcio), decoded to `str`
  (UTF-8, `errors="replace"`), held in RAM before timing starts. bs4 and htmlarc parse that
  `str`; **lxml is fed the UTF-8 bytes re-encoding of it** — its native input mode (see the
  footgun note below). On wikt (pure UTF-8) the inputs are semantically identical; on cc,
  lxml additionally honors each document's *declared* charset, so its DOM can differ
  slightly on mis-encoded pages.
- Workload: three selectors per document —
  `a[href]` (attribute extraction), `h1, h2, h3` (text), `table tr td:first-child` (text).
- Each phase runs in a fresh process; peak RSS via `ru_maxrss`. Gaps under 2× were
  confirmed with 3× interleaved A/B runs (spread ≈ ±4%; medians reported).

> **The lxml str footgun** (worth a sidebar in the article): `lxml.html.fromstring(s)`
> on a `str` raises `ValueError: Unicode strings with encoding declaration are not
> supported` for any document carrying an `<?xml … encoding=…?>` prolog — 66 of 5,000
> docs (1.3%) in this crawl sample, all perfectly valid XHTML. Feed lxml bytes and it's
> also ~13% faster. bs4 and htmlarc take `str` without complaint (bs4 also accepts raw
> bytes with charset detection; htmlarc is str-only).

**Correctness cross-check.** On wikt all three engines return *exactly* the same match
counts (111,303 / 43,286 / 2,172). On cc, htmlarc and bs4 agree within 0.004% on links
(629,227 vs 629,203) and exactly on headings (35,318); lxml sits ~2% lower on links and
cells (614,346 / 34,092), a mix of per-document charset interpretation and different
tree-recovery choices on malformed markup. Parse failures on cc: **htmlarc 0, bs4 0,
lxml 4** — and all 4 are degenerate documents (whitespace-only, or a lone comment), not
real HTML. On robustness the three are effectively at parity.

## Results

### Workflow 1 — one-shot extraction (parse + query each doc once, streaming)

| | wikt (9.6k docs, 52 MB) | cc (5k docs, 405 MB) | failures (cc) |
|---|---|---|---|
| BeautifulSoup | 8.25 s (6.3 MB/s) | 32.8 s (12 MB/s) | 0 |
| lxml | 0.64 s (82 MB/s) | 3.04 s (133 MB/s) | 4 |
| **htmlarc** | **0.48 s** (109 MB/s) | **2.50 s** (162 MB/s) | 0 |

One pass over the corpus: htmlarc is **1.2–1.3× faster than lxml** and **13–17× faster
than BeautifulSoup**. If you only ever look at the corpus once, lxml is entirely
competitive — this is not where htmlarc pulls away, and the article shouldn't pretend
otherwise.

### Workflow 2 — repeated analysis (ask the corpus a new question tomorrow)

bs4/lxml produce no artifact: every new session/question re-parses everything (= Workflow 1
again). htmlarc pays a one-time build, then every question hits a memory-mapped archive
with **zero HTML parsing**.

One-time build (parse + write `.htmlarc`):

| | build time | archive size | vs source HTML |
|---|---|---|---|
| wikt | 0.52 s | 68.5 MB | 1.31× |
| cc | 2.83 s | 274 MB | **0.68×** (smaller than the HTML) |

Cost of the *next* 3-selector sweep over the whole corpus, fresh process:

| | wikt | RSS | cc | RSS |
|---|---|---|---|---|
| BeautifulSoup (re-parse) | 8.25 s | — | 32.8 s | — |
| lxml (re-parse) | 0.64 s | — | 3.04 s | — |
| **htmlarc, Python loop (1 core)** | **0.10 s** | 108 MB | **0.43 s** | 418 MB |
| **htmlarc, `scan_*` (all cores, GIL released)** | **0.037 s** | 108 MB | **0.10 s** | 418 MB |

Opening the archive is sub-millisecond (mmap). Per question over the corpus:
**~7× faster than lxml / ~220–330× faster than bs4 single-threaded**, rising to
**17–30× vs lxml** with the parallel sweeps. The build cost amortizes after roughly one
re-query even against lxml. Note the sweeps do real work: the text selectors force each
matching document's zstd-compressed text block to inflate lazily — this is not a
topology-only cheat.

### Workflow 2b — the in-RAM alternative: hold all parsed trees

The only way bs4/lxml can skip re-parsing is keeping every tree alive in one long-running
process. That is both memory-expensive and *still slower to query* than htmlarc's mmap:

| query over pre-parsed corpus | wikt | RSS | cc | RSS |
|---|---|---|---|---|
| BeautifulSoup trees in RAM | 2.71 s | 1.25 GB | 10.5 s | 5.6 GB |
| lxml trees in RAM | 0.16 s | 0.83 GB | 0.97 s | 3.7 GB |
| **htmlarc mmap, loop (1 core)** | **0.10 s** | **0.11 GB** | **0.43 s** | **0.42 GB** |
| **htmlarc mmap, `scan_*`** | **0.037 s** | 0.11 GB | **0.10 s** | 0.42 GB |

lxml trees inflate the HTML ~8× in RAM (405 MB → 3.7 GB); bs4 ~14×. htmlarc queries the
on-disk archive about as fast as lxml queries its own in-memory trees single-threaded
(1.6–2.2×), and ~4–10× faster with the parallel sweeps, at ~8× lower RSS — and the
archive survives process exit, is shareable, and scales past RAM.

(Oneshot RSS columns omitted where dominated by the benchmark harness holding all input
HTML in RAM — identical across libraries, so absolute values aren't meaningful there.
A curiosity for the footnotes: bs4's hot-tree query is 4× slower than the same queries
interleaved with parsing, because millions of live Python objects mean cold caches and
expensive GC generations — lxml/htmlarc, whose trees live outside the Python heap,
show no such penalty.)

## Ergonomics

Task: *"every href in the corpus, with the document it came from."*

**BeautifulSoup** — re-parses per run; slowest; most forgiving/most documented API:

```python
from bs4 import BeautifulSoup

hrefs = []
for key, html in corpus:                    # re-parse, every run
    soup = BeautifulSoup(html, "lxml")
    for a in soup.select("a[href]"):
        hrefs.append((key, a["href"]))
```

**lxml** — fast parse, but selectors are bolted on (CSS→XPath via a separate package),
and the input rules have sharp edges (bytes vs str, empty docs raise):

```python
import lxml.html
from lxml.cssselect import CSSSelector

links = CSSSelector("a[href]")              # compile once
hrefs = []
for key, html in corpus:                    # re-parse, every run
    try:
        tree = lxml.html.fromstring(html.encode())  # str input raises on XHTML prologs
    except lxml.etree.ParserError:
        continue                            # whitespace-only docs raise too
    for a in links(tree):
        hrefs.append((key, a.get("href")))
```

**htmlarc** — parse once into an archive, then every question is two lines:

```python
import htmlarc

# once:
b = htmlarc.ArchiveBuilder()
for key, html in corpus:
    b.add(key, html)
b.write("corpus.htmlarc")

# every analysis after that:
arc = htmlarc.open("corpus.htmlarc")        # mmap, sub-millisecond
hrefs = [(key, h) for key, hs in arc.scan_attr("a[href]", "href")
                  for h in hs if h]         # all cores, GIL released
```

### Where each is nicer

**htmlarc**
- The archive is a *durable, shareable artifact*: repeated analyses, notebooks that restart,
  team members querying the same file — nobody re-parses.
- Corpus-level verbs (`matching`, `scan_text`, `scan_attr`, `Filter`) — the "sweep every
  document" loop you'd hand-write disappears, and parallelism is free (GIL released).
  With bs4/lxml, using multiple cores means `multiprocessing` + re-parsing in each worker.
- Batch per-doc calls (`select_text`, `select_attr`, `select_html`) avoid per-element
  round-trips; ships type stubs.
- Accepts any string without raising (HTML5-style recovery; 0 failures here and ~0.0005%
  on the project's 2M-doc crawl runs, those being capacity limits, not markup).
- API is small and reads like modern bs4 (`select`, `select_first`, `el.text`,
  `el["href"]`, `el.classes`) — low switching cost from bs4 for read-only work.

**BeautifulSoup**
- Enormous ecosystem, docs, and Stack Overflow surface; every scraping tutorial targets it.
- Encoding detection built in (`UnicodeDammit`) — feed it raw bytes; htmlarc requires
  pre-decoded `str`, lxml wants bytes.
- Full mutation API (edit, prettify, unwrap) and pluggable parsers.

**lxml**
- XPath — strictly more expressive than CSS for some extractions; plus XSLT, serialization,
  full mutation, and decades of maturity.
- Fastest incumbent, and robust in practice (its 4 cc failures were empty/comment-only
  documents); the sane default if you don't need bs4's ecosystem or an artifact.

### Honest htmlarc caveats (for the article)

- The Python API is **read/query-only**: no DOM mutation from Python (that exists in the
  Rust API). If your pipeline rewrites HTML, it's the wrong tool from Python today.
- **Not on PyPI yet** — built from source with maturin for this benchmark. A `pip install`
  story is a prerequisite for the article's readers to follow along.
- No encoding handling at all: you decode bytes to `str` yourself (bs4 detects charsets;
  lxml honors declared ones).
- No XPath; selector language is CSS3 (though `:has()`, `nth-*`, attribute operators all
  work — as they do in soupsieve and cssselect 1.4, so expressiveness across the three is
  closer than you'd expect).
- One-shot gains over lxml are modest (1.2–1.3×); the order-of-magnitude wins are
  specifically in the parse-once-query-many workflow, and on small-doc corpora the archive
  is 1.3× the source HTML (it's a pre-parsed DOM store, not a compressor — though on
  general web HTML it came out 0.68× thanks to per-bundle zstd of text).
- The parallel `scan_*` speedup over the single-core loop here is 2.6–4.3×, not ~10×:
  these sweeps finish in 37–100 ms, so Python-side result marshalling (which still holds
  the GIL) is a large fraction. Heavier extractions parallelize better.

## Reproduce

From this directory (needs the local measurement corpus — see "Measurement corpus" in
the repository README, or set `HTMLARC_CORPUS=/path/to/corpus`):

```sh
uv venv -p 3.12 bench-venv
uv pip install -p bench-venv/bin/python beautifulsoup4 lxml cssselect warcio libzim
uvx maturin build --release -m ../../crates/htmlarc-py/Cargo.toml -i bench-venv/bin/python
uv pip install -p bench-venv/bin/python ../../target/wheels/htmlarc-*.whl

bench-venv/bin/python extract.py wikt      # -> data/wikt.pkl
bench-venv/bin/python extract.py cc        # -> data/cc.pkl

# One phase per process, one JSON line each; e.g.:
bench-venv/bin/python bench.py oneshot_bs4 wikt
bench-venv/bin/python bench.py build_htmlarc wikt    # required before requery_htmlarc
bench-venv/bin/python bench.py requery_htmlarc wikt
```
