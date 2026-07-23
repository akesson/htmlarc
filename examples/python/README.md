# Runnable Python recipes

Self-contained scripts showing htmlarc on real, freely downloadable data. Each
recipe fetches what it needs into `data/` (gitignored, nothing redistributed
here), keeps downloads small (streamed samples, single-record range requests),
and makes one point:

| Recipe | Point | Data / first run |
|---|---|---|
| [`warc_to_archive.py`](warc_to_archive.py) | Common Crawl WARC → archive **+ metadata sidecar parquet**; correct bytes→str decoding | streams ~500 docs (~35 MB) from the latest crawl — well under a minute |
| [`corpus_questions.py`](corpus_questions.py) | three corpus questions answered in milliseconds, sidecar join included | archive from recipe 1; runs in seconds |
| [`site_audit.py`](site_audit.py) | SEO audit table + a question invented *after* the crawl (no re-crawl) | polite ~60-page crawl of books.toscrape.com, ~1 min |
| [`snapshot_diff.py`](snapshot_diff.py) | same site from two crawls a year apart, diffed with a polars join; archive-per-batch pattern | CDX index + ~200 single-record range requests, ~2 min |
| [`zim_wiktionary.py`](zim_wiktionary.py) | ZIM → structured dictionary via CSS on rendered HTML (no wikitext templates) | ~4 MB Wiktionary ZIM from Kiwix; converts in ~2 s |
| [`rag_chunking.py`](rag_chunking.py) | heading-aware, boilerplate-free RAG chunks ("select in, don't strip out") | archive from recipe 1; runs in seconds |
| [`extractor_from_archive.py`](extractor_from_archive.py) | LLM-pipeline loop: re-run extractors without re-reading the crawl | archive from recipe 1; runs in seconds |

## Running

Each script declares its dependencies inline (PEP 723), so [uv] runs them
directly. Until htmlarc is on PyPI, build the wheel once and pass it along:

```sh
uvx maturin build --release -m ../../crates/htmlarc-py/Cargo.toml
uv run --with ../../target/wheels/htmlarc-*.whl warc_to_archive.py
```

(Once `pip install htmlarc` exists, the `--with` goes away and these are plain
`uv run <recipe>.py`.)

Recipes 2, 6 and 7 build the recipe-1 archive automatically if it's missing;
`--limit N` controls corpus size where applicable. Everything lands in `data/`;
delete it to start fresh.

Lifting a recipe into your own project? They all import [`_common.py`](_common.py)
— 40 lines of `requests` plumbing (data dir, polite retrying GET, charset
decoding). Copy it along, or inline the two helpers.

## Editor completions

`uv run` builds its environment at run time, so language servers can't see the
recipes' dependencies. For completions and type hints, run `make py-dev` at the
repo root — it creates a `.venv` with the htmlarc wheel (typed via its shipped
stub) plus every recipe dependency. Point your editor at that interpreter.

With the tracked hooks active (`make setup`), the venv refreshes itself after
every pull, merge, and branch switch (~1–4 s, only when `.venv` exists). After
*local* Rust edits, re-run `make py-dev` by hand — git hooks can't see those.

[uv]: https://docs.astral.sh/uv/
