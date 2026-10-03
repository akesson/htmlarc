# htmlarc 0.1.1 release measurements (format v12)

Measured 2026-10-03 with htmlarc 0.1.1 **as published on PyPI**
(`htmlarc-0.1.1-cp310-abi3-macosx_11_0_arm64.whl`), lxml 6.1.3, cssselect 1.5.0,
BeautifulSoup 4.15.0 + soupsieve 2.10, pyarrow 25.0.1, warcio 1.8.1, CPython 3.12.13,
Apple M4 Pro (10P+4E, 14 logical CPUs), 48 GB, macOS 27.0.1. Each cell is the
**median of three separate processes**, in seconds. Every phase of every workflow in
the [v11 README](README.md) was re-measured. Raw runs, environment, input hashes,
counts and failures are in [release-0.1.1.json](release-0.1.1.json).

| Corpus | Documents | HTML (UTF-8) | `.htmlarc` v12 | Archive / HTML |
|---|---:|---:|---:|---:|
| wikt (Corsican Wiktionary) | 9,567 | 52.3 MB | 68.5 MB | 1.31× |
| cc (Common Crawl `cc_000`, first 5,000 HTML 200s) | 5,000 | 417.4 MB | 277.4 MB | 0.66× |

The input pickles hash-match the [release-v12](release-v12.md) run. All runs are
**warm-cache** (the input is fully in the page cache, verified with `mincore`)
unless a row says *cold*. Core counts are stated in each row. "14 cores" means 14
worker processes for lxml (`multiprocessing`, fork) and 14 threads for htmlarc
(`scan_*` uses `available_parallelism()` = 14).

## 1. One-shot extraction (parse + 3 selectors, each document once, 1 core)

| | wikt | cc | parse failures (cc) |
|---|---:|---:|---:|
| BeautifulSoup (lxml backend) | 7.306 | 29.221 | 0 |
| lxml | 0.618 | 3.054 | 4 |
| **htmlarc** | **0.459** | **2.421** | 0 |
| htmlarc vs lxml | 1.35× | 1.26× | |
| htmlarc vs BeautifulSoup | 15.9× | 12.1× | |

If you look at a corpus once, htmlarc is only about 1.3× faster than lxml.

## 2. Build (one time: parse + write `.htmlarc`, 1 core)

| | wikt | cc |
|---|---:|---:|
| Build time | 0.531 | 2.783 |
| Build time as a fraction of one lxml pass | 0.86× | 0.91× |

## 3. Requery: the next 3-selector extraction over the whole corpus

htmlarc rows open the archive in a fresh process (opening is included and takes
under 0.2 ms). lxml has no artifact, so it re-parses.

| | cores | wikt | cc | RSS (cc) |
|---|---:|---:|---:|---:|
| lxml, re-parse | 1 | 0.618 | 3.054 | † |
| lxml, re-parse, `ThreadPool` | 14 | 0.722 | 2.423 | † |
| lxml, re-parse, `multiprocessing` | 14 | 0.111 | 0.415 | † |
| **htmlarc, Python loop** (`select_attr`/`select_text`) | 1 | **0.091** | **0.345** | 0.42 GB |
| **htmlarc, `scan_*`** | 14 | **0.036** | **0.087** | 0.39 GB |
| **htmlarc, `scan_table` → pyarrow** | 14 | **0.030** | **0.046** | 0.48 GB |

Ratios, same cores on both sides:

| | wikt | cc |
|---|---:|---:|
| 1 core: htmlarc loop vs lxml | 6.8× | 8.9× |
| 14 cores: htmlarc `scan_*` vs lxml `multiprocessing` | 3.1× | 4.8× |
| 14 cores: htmlarc `scan_table` vs lxml `multiprocessing` | 3.8× | 9.1× |

Mixed core counts, labelled as such: htmlarc on 1 core still beats lxml on 14
(1.2× on both corpora), and htmlarc `scan_*` on 14 is 17× (wikt) / 35× (cc)
faster than lxml on 1 core.

## 4. Counting: the same three questions answered as counts

lxml uses XPath `count()` inside libxml2. htmlarc uses `select_count` /
`scan_count` inside Rust. Neither marshals a match into Python.

| | cores | wikt | peak RSS | cc | peak RSS |
|---|---:|---:|---:|---:|---:|
| lxml `count()`, re-parse | 1 | 0.548 | † | 2.785 | † |
| lxml `count()`, re-parse, `multiprocessing` | 14 | 0.102 | 0.56 GB ‡ | 0.402 | 2.32 GB ‡ |
| **htmlarc `select_count` loop** | 1 | **0.0495** | 0.10 GB | **0.179** | 0.23 GB |
| **htmlarc `scan_count`** | 14 | **0.0121** | 0.10 GB | **0.0199** | 0.23 GB |

| | wikt | cc |
|---|---:|---:|
| **14 vs 14 cores: `scan_count` vs lxml `multiprocessing`** | **8.4×** | **20×** |
| 1 vs 1 core: `select_count` loop vs lxml | 11× | 16× |
| 14 vs 1 core (mixed, not a fair comparison) | 45× | 140× |

The fair headline is the 14-vs-14 row: **about 20× on cc and 8× on wikt**.

## 5. Holding every parsed tree in RAM (the in-process alternative to re-parsing)

| query over pre-parsed corpus | cores | wikt | RSS | cc | RSS |
|---|---:|---:|---:|---:|---:|
| BeautifulSoup trees | 1 | 1.753 | 1.25 GB | 6.728 | 5.55 GB |
| lxml trees | 1 | 0.163 | 0.83 GB | 1.061 | 3.68 GB |
| **htmlarc mmap, loop** | 1 | **0.091** | 0.12 GB | **0.345** | 0.42 GB |
| **htmlarc mmap, `scan_*`** | 14 | **0.036** | 0.12 GB | **0.087** | 0.39 GB |

Same core count (1 vs 1), htmlarc querying the archive is 1.8× (wikt) / 3.1× (cc)
faster than lxml querying its own live trees. The RSS columns include the
harness holding the input corpus in RAM. Net of that (subtracting one-shot lxml's
RSS), lxml's trees cost about 0.66 GB (wikt) / 2.3 GB (cc), against htmlarc's
0.12 / 0.42 GB: about 5.5× less. Building the trees costs 0.42 s / 1.99 s for lxml
and 7.7 s / 36.4 s for BeautifulSoup.

## 6. WARC pipeline: per question, straight from `cc_000.warc.gz` (cc only)

| | cores | time | peak RSS |
|---|---:|---:|---:|
| warcio read + decode only, no parsing (the floor) | 1 | 0.710 | 0.05 GB |
| lxml pipeline | 1 | 3.762 | 0.07 GB |
| lxml pipeline, fork + `imap` | 14 | 0.875 | 1.49 GB ‡ |
| BeautifulSoup pipeline (charset detection) | 1 | 30.135 | 0.10 GB |
| **htmlarc `scan_*` on the converted archive** | 14 | **0.087** | 0.39 GB |
| **htmlarc `scan_count`** | 14 | **0.020** | 0.23 GB |

Reading and decoding the source alone costs 8.1× htmlarc's whole extraction. 14-core
lxml reaches 0.875 s, close to that serial-read floor, and is 10× slower than
`scan_*` (14 vs 14). The pipeline is CPU-bound: the cold-cache runs (section 9) are
within 1% of these warm ones. Raw-bytes input lets lxml/bs4 honour declared
charsets, so their counts shift by about 1% from the in-RAM tables (lxml
623,428 / 35,040 / 36,726, 3 failures).

## 7. Arrow sweep (`requery_htmlarc_arrow`)

The three extractions as three `scan_table` Arrow tables, handed to pyarrow
zero-copy: **0.030 s (wikt) / 0.046 s (cc)**, 14 cores. That is 1.2× / 1.9× faster
than the `scan_*` list sweeps, because columns are built off-GIL. The pyarrow
handover itself takes 0.2 ms.

## 8. New phase: a question nobody planned at build time

This is the [Common Crawl notebook](../../examples/python/common_crawl.ipynb)'s
"question you think of tomorrow": how many pages ship JSON-LD
(`script[type="application/ld+json"]`), and how many links use plain http
(`a[href^='http://']`). htmlarc uses `matching` + `scan_count`. lxml uses XPath
`boolean()` + `count()` in libxml2.

| cc | cores | time | peak RSS | pages with JSON-LD | http links |
|---|---:|---:|---:|---:|---:|
| lxml, re-parse | 1 | 2.395 | † | 1,099 | 246,971 |
| lxml, re-parse, `multiprocessing` | 14 | 0.360 | 2.32 GB ‡ | 1,099 | 246,971 |
| **htmlarc loop** | 1 | **0.089** | 0.25 GB | 1,101 | 252,046 |
| **htmlarc `matching` + `scan_count`** | 14 | **0.019** | 0.25 GB | 1,101 | 252,046 |

Comparing equal core counts: **19× (14 vs 14)**, 27× (1 vs 1). On wikt neither
selector matches anything (0 / 0). The cost there is 0.104 s for 14-core lxml and
0.014 s for htmlarc (7.6×), but the answer is empty, so cite cc. The htmlarc loop
row runs after the sweep in the same process.

## 9. New phase: cold cache (the archive not in the page cache)

For a cold run, the input is evicted before timing by rewriting it through an
`F_NOCACHE` copy. That needs no root, unlike `purge`. `mincore` then confirms that
0% of the file is resident. lxml's in-RAM rows above have no cold equivalent: their
input is already decoded in memory. The cold-vs-cold comparison is the WARC
pipeline.

| | cores | wikt warm | wikt cold | cc warm | cc cold |
|---|---:|---:|---:|---:|---:|
| htmlarc loop, extract | 1 | 0.091 | 0.150 | 0.345 | 0.576 |
| htmlarc loop, count | 1 | 0.0495 | 0.118 | 0.179 | 0.373 |
| htmlarc `scan_*`, extract | 14 | 0.036 | 0.106 | 0.087 | 0.211 |
| htmlarc `scan_count` | 14 | 0.0121 | 0.076 | 0.0199 | 0.113 |
| htmlarc `scan_table` | 14 | 0.030 | 0.095 | 0.046 | 0.171 |
| htmlarc new question | 14 | 0.014 | 0.077 | 0.019 | 0.120 |
| lxml WARC pipeline | 14 | | | 0.875 | 0.876 |

A cold archive costs 2.4–6.3× more on the 14-core sweeps and 1.6–2.4× more on the
1-core loops. Even so, compared with
14-core lxml:

- on the pipeline (cold vs cold, cc): `scan_*` is 4.2× faster and `scan_count`
  7.8× faster;
- against lxml's warm, in-RAM re-parse, a cold `scan_count` is still 3.6× faster
  on cc, but only 1.3× on wikt, and a cold wikt `scan_*` is level (1.05×).

The cold penalty is mostly page-fault latency, not disk bandwidth. In an ad-hoc
check (not in the JSON), reading the evicted archive sequentially first, then
scanning, took 0.027 s (wikt) / 0.079 s (cc) in total. A cold `scan_count` takes
0.076 / 0.113 s. A prefetch in htmlarc would close most of that gap.

## Caveats (keep these with any number quoted)

- **One-shot is not where htmlarc wins**: 1.26–1.35× over lxml. The large ratios
  are for parse-once, query-many workflows and assume the one-time build in
  section 2.
- **State the core counts.** Ratios that mix 1-core lxml with 14-core htmlarc
  (e.g. 140× on counting) are labelled as mixed and are not fair comparisons.
- **cc match counts differ between parsers.** htmlarc 629,227 / 35,318 / 37,481,
  bs4 629,203 / 35,318 / 37,609, lxml 614,346 / 34,707 / 34,092. Tree recovery
  and charset handling differ, so cc timings are not correctness-equivalent. wikt
  counts agree exactly across all three (111,303 / 43,286 / 2,172). Parse failures
  on cc: htmlarc 0, bs4 0, lxml 4 (degenerate documents).
- **Runs are warm-cache** unless marked cold. Each htmlarc sweep reads text from
  the mmap; lxml rows start from HTML already decoded in RAM.
- † In-RAM lxml/bs4 RSS is dominated by the harness holding the corpus (as `str`
  and as `bytes`), so absolute values are not meaningful. ‡ `multiprocessing`
  peak RSS is the summed RSS of the parent and its workers, taken from a second,
  untimed pass. It counts fork's copy-on-write pages once per process, so it
  overstates physical memory. The pipeline row (1.49 GB) is the cleanest
  comparison, because its parent does not hold the corpus.
- **Noise.** The machine was not idle: load average 2.3–4.5, with OrbStack using
  0.4–0.65 of a core throughout. Min-to-max spread across the three runs is within
  5% for most steps above 30 ms, but up to 8–14% for the lxml `multiprocessing`,
  hot-lxml and cold htmlarc rows, and 15–30% for steps under 30 ms. Session drift is about ±10%.
  An interleaved A/B a few minutes later measured the cc count loop at 0.163 s
  against 0.179 s here, and found the PyPI wheel and a local `--release` build of
  `v0.1.1` within 1% of each other.

## Compared with earlier runs

Format v11 (README, July 2026): htmlarc's own times are equal or better (cc loop
0.39 → 0.345, cc `scan_*` 0.10 → 0.087, cc build 3.19 → 2.78). **Some ratios
against lxml are smaller**, because lxml's `multiprocessing` rows are faster on
this run (wikt 0.15 → 0.111, cc 0.52 → 0.415; count 0.13 → 0.102 and
0.46 → 0.402). lxml/cssselect, macOS and background load all differ between the
runs, so the cause can't be attributed.

| ratio | v11 | 0.1.1 |
|---|---:|---:|
| count, 14 vs 14, cc / wikt | ~25× / ~10× | 20× / 8.4× |
| count, 14 htmlarc vs 1 lxml, cc | ~170× | 140× |
| extract `scan_*`, 14 vs 14, cc / wikt | 4–5× | 4.8× / 3.1× |
| htmlarc 1 core vs lxml 14, cc / wikt | 1.3× / 1.7× | 1.2× / 1.2× |

BeautifulSoup also got faster (cc one-shot 32.8 → 29.2 s, hot query 10.5 → 6.7 s;
soupsieve 2.8.4 → 2.10). Compared with the 0.1.0 [release-v12](release-v12.md) run, all
shared rows are within the ±10% drift band. 0.1.1 has no code changes from 0.1.0.

## Reproduce

```sh
uv venv -p 3.12 bench-venv
uv pip install -p bench-venv/bin/python htmlarc==0.1.1 lxml==6.1.3 cssselect==1.5.0 \
    beautifulsoup4 warcio libzim pyarrow psutil
export HTMLARC_BENCH_DATA="$PWD/target/release-0.1.1-benchmark-data"
export HTMLARC_CORPUS="$HTMLARC_BENCH_DATA"    # holds a copy of cc_000.warc.gz:
cp corpus/cc_000.warc.gz "$HTMLARC_CORPUS/"    # cold runs rewrite it to evict it
bench-venv/bin/python benchmarks/python-compare/extract.py wikt
bench-venv/bin/python benchmarks/python-compare/extract.py cc
bench-venv/bin/python benchmarks/python-compare/run_suite.py release-0.1.1.json
```

`run_suite.py` runs every phase in its own process for 3 repeats, warm phases
first and then the cold ones. Cold mode works on macOS only (`F_NOCACHE`).
