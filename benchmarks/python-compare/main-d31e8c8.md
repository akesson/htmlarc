# htmlarc main (`d31e8c8`) measurements (format v12)

Measured 2026-10-05 with htmlarc built from `main` at `d31e8c8` (a local
`maturin build --release`; the package version string still reads 0.1.1, but the
build includes PRs #100–#104, which are **not on PyPI**). Same stack as the 0.1.1
run: lxml 6.1.3, cssselect 1.5.0, BeautifulSoup 4.15.0 + soupsieve 2.10, pyarrow
25.0.1, warcio 1.8.1, CPython 3.12.13, Apple M4 Pro (10P+4E, 14 logical CPUs),
48 GB, macOS 27.0.1, on mains power.
Each cell is the **median of three separate processes**, in seconds. Every phase
of every workflow in the [v11 README](README.md) was re-measured. Raw runs,
environment, input hashes, counts and failures are in
[main-d31e8c8.json](main-d31e8c8.json), together with the raw runs of the A/B
against the PyPI 0.1.1 wheel and the cold read-first check.

| Corpus | Documents | HTML (UTF-8) | `.htmlarc` v12 | Archive / HTML |
|---|---:|---:|---:|---:|
| wikt (Corsican Wiktionary) | 9,567 | 52.3 MB | 68.5 MB | 1.31× |
| cc (Common Crawl `cc_000`, first 5,000 HTML 200s) | 5,000 | 417.4 MB | 277.4 MB | 0.66× |

The input pickles hash-match the 0.1.1 and [release-v12](release-v12.md) runs, and the
archives main builds are byte-identical to 0.1.1's. All runs are **warm-cache** (the
input is fully in the page cache, verified with `mincore`) unless a row says
*cold*. Core counts are stated in each row. "14 cores" means 14 worker processes
for lxml (`multiprocessing`, fork) and 14 threads for htmlarc (`scan_*` uses
`available_parallelism()` = 14).

## 1. One-shot extraction (parse + 3 selectors, each document once, 1 core)

| | wikt | cc | parse failures (cc) |
|---|---:|---:|---:|
| BeautifulSoup (lxml backend) | 7.207 | 29.208 | 0 |
| lxml | 0.624 | 3.006 | 4 |
| **htmlarc** | **0.328** | **1.537** | 0 |
| htmlarc vs lxml | 1.90× | 1.96× | |
| htmlarc vs BeautifulSoup | 22× | 19× | |

If you look at a corpus once, htmlarc is about 1.9× faster than lxml.

## 2. Build (one time: parse + write `.htmlarc`, 1 core)

| | wikt | cc |
|---|---:|---:|
| Build time | 0.434 | 2.023 |
| Build time as a fraction of one lxml pass | 0.70× | 0.67× |

## 3. Requery: the next 3-selector extraction over the whole corpus

htmlarc rows open the archive in a fresh process (opening is included and takes
under 0.2 ms). lxml has no artifact, so it re-parses.

| | cores | wikt | cc | RSS (cc) |
|---|---:|---:|---:|---:|
| lxml, re-parse | 1 | 0.624 | 3.006 | † |
| lxml, re-parse, `ThreadPool` | 14 | 0.689 | 2.383 | † |
| lxml, re-parse, `multiprocessing` | 14 | 0.114 | 0.447 | † |
| **htmlarc, Python loop** (`select_attr`/`select_text`) | 1 | **0.057** | **0.200** | 0.42 GB |
| **htmlarc, `scan_*`** | 14 | **0.020** | **0.072** | 0.39 GB |
| **htmlarc, `scan_table` → pyarrow** | 14 | **0.012** | **0.030** | 0.48 GB |

Ratios, same cores on both sides:

| | wikt | cc |
|---|---:|---:|
| 1 core: htmlarc loop vs lxml | 11× | 15× |
| 14 cores: htmlarc `scan_*` vs lxml `multiprocessing` | 5.7× | 6.2× |
| 14 cores: htmlarc `scan_table` vs lxml `multiprocessing` | 9.8× | 15× |

Mixed core counts, labelled as such: htmlarc on 1 core beats lxml on 14
(2.0× wikt, 2.2× cc), and htmlarc `scan_*` on 14 is 31× (wikt) / 42× (cc)
faster than lxml on 1 core.

## 4. Counting: the same three questions answered as counts

lxml uses XPath `count()` inside libxml2. htmlarc uses `select_count` /
`scan_count` inside Rust. Neither marshals a match into Python.

| | cores | wikt | peak RSS | cc | peak RSS |
|---|---:|---:|---:|---:|---:|
| lxml `count()`, re-parse | 1 | 0.540 | † | 2.781 | † |
| lxml `count()`, re-parse, `multiprocessing` | 14 | 0.107 | 0.57 GB ‡ | 0.401 | 2.32 GB ‡ |
| **htmlarc `select_count` loop** | 1 | **0.0187** | 0.10 GB | **0.0507** | 0.23 GB |
| **htmlarc `scan_count`** | 14 | **0.0033** | 0.10 GB | **0.0077** | 0.23 GB |

| | wikt | cc |
|---|---:|---:|
| **14 vs 14 cores: `scan_count` vs lxml `multiprocessing`** | **32×** | **52×** |
| 1 vs 1 core: `select_count` loop vs lxml | 29× | 55× |
| 14 vs 1 core (mixed, not a fair comparison) | 163× | 361× |

The fair headline is the 14-vs-14 row: **about 50× on cc and 30× on wikt**. The
htmlarc side is a few milliseconds, where a fraction of a millisecond moves the
ratio by 10%, so quote these as "about".

## 5. Holding every parsed tree in RAM (the in-process alternative to re-parsing)

| query over pre-parsed corpus | cores | wikt | RSS | cc | RSS |
|---|---:|---:|---:|---:|---:|
| BeautifulSoup trees | 1 | 1.785 | 1.25 GB | 6.801 | 5.29 GB |
| lxml trees | 1 | 0.181 | 0.83 GB | 1.028 | 3.71 GB |
| **htmlarc mmap, loop** | 1 | **0.057** | 0.12 GB | **0.200** | 0.42 GB |
| **htmlarc mmap, `scan_*`** | 14 | **0.020** | 0.12 GB | **0.072** | 0.39 GB |

Same core count (1 vs 1), htmlarc querying the archive is 3.2× (wikt) / 5.1× (cc)
faster than lxml querying its own live trees. The RSS columns include the
harness holding the input corpus in RAM. Net of that (subtracting one-shot lxml's
RSS), lxml's trees cost about 0.66 GB (wikt) / 2.35 GB (cc), against htmlarc's
0.12 / 0.42 GB: about 5.5× less. Building the trees costs 0.43 s / 1.96 s for
lxml and 7.7 s / 36.9 s for BeautifulSoup.

## 6. WARC pipeline: per question, straight from `cc_000.warc.gz` (cc only)

| | cores | time | peak RSS |
|---|---:|---:|---:|
| warcio read + decode only, no parsing (the floor) | 1 | 0.719 | 0.05 GB |
| lxml pipeline | 1 | 3.816 | 0.07 GB |
| lxml pipeline, fork + `imap` | 14 | 0.884 | 1.54 GB ‡ |
| BeautifulSoup pipeline (charset detection) | 1 | 29.528 | 0.10 GB |
| **htmlarc `scan_*` on the converted archive** | 14 | **0.072** | 0.39 GB |
| **htmlarc `scan_count`** | 14 | **0.0077** | 0.23 GB |

Reading and decoding the source alone costs 10× htmlarc's whole extraction. 14-core
lxml reaches 0.884 s, close to that serial-read floor, and is 12× slower than
`scan_*` (14 vs 14). The pipeline is CPU-bound: the cold-cache runs (section 9) are
within 2% of these warm ones. Raw-bytes input lets lxml/bs4 honour declared
charsets, so their counts shift by about 1% from the in-RAM tables (lxml
623,428 / 35,040 / 36,726, 3 failures).

## 7. Arrow sweep (`requery_htmlarc_arrow`)

The three extractions as three `scan_table` Arrow tables, handed to pyarrow
zero-copy: **0.012 s (wikt) / 0.030 s (cc)**, 14 cores. That is 1.7× / 2.4× faster
than the `scan_*` list sweeps, because columns are built off-GIL. The pyarrow
handover itself takes either 0.2 ms or about 2.6 ms, depending on the process
(bimodal on every run: the first of the three processes pays 2.6 ms, the
next two 0.2 ms; the cause was not investigated).

## 8. New phase: a question nobody planned at build time

This is the [Common Crawl notebook](../../examples/python/common_crawl.ipynb)'s
"question you think of tomorrow": how many pages ship JSON-LD
(`script[type="application/ld+json"]`), and how many links use plain http
(`a[href^='http://']`). htmlarc uses `matching` + `scan_count`. lxml uses XPath
`boolean()` + `count()` in libxml2.

| cc | cores | time | peak RSS | pages with JSON-LD | http links |
|---|---:|---:|---:|---:|---:|
| lxml, re-parse | 1 | 2.432 | † | 1,099 | 246,971 |
| lxml, re-parse, `multiprocessing` | 14 | 0.360 | 2.31 GB ‡ | 1,099 | 246,971 |
| **htmlarc loop** | 1 | **0.052** | 0.26 GB | 1,101 | 252,046 |
| **htmlarc `matching` + `scan_count`** | 14 | **0.016** | 0.26 GB | 1,101 | 252,046 |

Comparing equal core counts: **23× (14 vs 14)**, 47× (1 vs 1). The htmlarc sweep
spread 12–16 ms over the three processes; against its median and slowest run
the ratio is 22–23×.
On wikt neither selector matches anything (0 / 0). The cost there is 0.105 s for
14-core lxml and 0.0064 s for htmlarc (16×), but the answer is empty, so cite cc.
The htmlarc loop row runs after the sweep in the same process.

## 9. New phase: cold cache (the archive not in the page cache)

For a cold run, the input is evicted before timing by rewriting it through an
`F_NOCACHE` copy. That needs no root, unlike `purge`. `mincore` then confirms that
0% of the file is resident. lxml's in-RAM rows above have no cold equivalent: their
input is already decoded in memory. The cold-vs-cold comparison is the WARC
pipeline.

| | cores | wikt warm | wikt cold | cc warm | cc cold |
|---|---:|---:|---:|---:|---:|
| htmlarc loop, extract | 1 | 0.057 | 0.124 | 0.200 | 0.443 |
| htmlarc loop, count | 1 | 0.0187 | 0.092 | 0.0507 | 0.259 |
| htmlarc `scan_*`, extract | 14 | 0.020 | 0.042 | 0.072 | 0.153 |
| htmlarc `scan_count` | 14 | 0.0033 | 0.023 | 0.0077 | 0.072 |
| htmlarc `scan_table` | 14 | 0.012 | 0.034 | 0.030 | 0.114 |
| htmlarc new question | 14 | 0.0064 | 0.026 | 0.016 | 0.086 |
| lxml WARC pipeline | 14 | | | 0.884 | 0.896 |

A cold archive costs 2.1–9.3× more on the 14-core sweeps and 2.2–5.1× more on the
1-core loops. The largest multiples are on the fastest warm steps (`scan_count`),
where reading the file dominates. Compared with 14-core lxml:

- on the pipeline (cold vs cold, cc): `scan_*` is 5.9× faster and `scan_count`
  12× faster;
- against lxml's warm, in-RAM re-parse, a cold `scan_count` is still 5.6× faster
  on cc and 4.7× on wikt, and a cold `scan_*` is 2.9× (cc) / 2.7× (wikt) faster.

A cold `scan_count` runs close to the speed of a sequential read. Reading the
evicted archive sequentially first and then scanning takes 0.018 s (wikt) /
0.067 s (cc) in total; the read alone is 0.014 / 0.059 s (4.7–4.9 GB/s). A cold
`scan_count` takes 0.023 / 0.072 s, so page faults cost about 5 ms more than an
up-front read. On 0.1.1 the same check gave 0.027 / 0.079 s against a cold
`scan_count` of 0.076 / 0.113 s (a 34–49 ms gap). A prefetch in htmlarc could
now save at most those 5 ms.

## Caveats (keep these with any number quoted)

- **Not released.** These numbers are for `main`, not the PyPI 0.1.1 wheel. Until a
  release ships PRs #100–#104, `pip install htmlarc` gives the 0.1.1 numbers in
  the A/B table below.
- **One-shot is not where htmlarc wins most**: 1.90–1.96× over lxml. The large ratios
  are for parse-once, query-many workflows and assume the one-time build in
  section 2.
- **State the core counts.** Ratios that mix 1-core lxml with 14-core htmlarc
  (e.g. 361× on counting) are labelled as mixed and are not fair comparisons.
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
  overstates physical memory. The pipeline row (1.54 GB here, 1.49–1.66 GB on earlier
  runs: the sampler's variance) is the cleanest comparison, because its parent
  does not hold the corpus.
- **Noise.** Each phase ran under a shared CPU lock that waits for the 1-minute
  load average to drop below 3 (it was 2.3–3.0 at each phase start). Most steps
  spread within 6% over their three processes. The widest spreads are the wikt
  `scan_table` sweep (36%: one process took 15 ms against 11 ms), the cc
  new-question sweep (24%), the build's write step (20–22%) and the hot-lxml query
  (17–19%). An earlier full run of this build on battery, the same day, agreed:
  every headline ratio against lxml stayed within 8%, except the wikt `scan_table`
  ratio (8.0× on battery), which moved with the bimodal pyarrow handover
  (section 7). The 14-process lxml rows on battery were within 10% of these, so
  the battery caveat that run carried turned out to be unnecessary.

## Compared with the PyPI 0.1.1 wheel

Interleaved A/B on the same archive, right after the suite: each round runs the
phase with the PyPI wheel, then with the main build, in fresh processes, 3 rounds.
The per-round ratios agree within ±0.15×, except cc `scan_count` (2.35–3.28×).

| phase (fresh process) | 0.1.1 | main | speedup |
|---|---:|---:|---:|
| one-shot, cc | 2.375 | 1.521 | 1.56× |
| loop extract, cc | 0.333 | 0.199 | 1.67× |
| `scan_*` extract, wikt | 0.036 | 0.020 | 1.80× |
| `scan_*` extract, cc | 0.084 | 0.068 | 1.24× |
| `scan_table`, wikt | 0.030 | 0.011 | 2.7× |
| `scan_table`, cc | 0.045 | 0.030 | 1.48× |
| loop count, cc | 0.174 | 0.053 | 3.3× |
| `scan_count`, wikt | 0.0120 | 0.0033 | 3.6× |
| `scan_count`, cc | 0.0196 | 0.0076 | 2.6× |
| new question loop, cc | 0.089 | 0.050 | 1.79× |
| new question `scan_count`, cc | 0.0202 | 0.0155 | 1.30× |

Headline ratios against lxml (14 vs 14 unless stated):

| ratio | v11 | 0.1.1 | main |
|---|---:|---:|---:|
| one-shot, 1 vs 1, cc / wikt | | 1.26× / 1.35× | 1.96× / 1.90× |
| count, cc / wikt | ~25× / ~10× | 20× / 8.4× | 52× / 32× |
| extract `scan_*`, cc / wikt | 4–5× | 4.8× / 3.1× | 6.2× / 5.7× |
| extract `scan_table`, cc / wikt | | 9.1× / 3.8× | 15× / 9.8× |
| new question, cc | | 19× | 23× |
| WARC pipeline vs `scan_*`, cc | | 10× | 12× |
| htmlarc 1 core vs lxml 14, cc / wikt | 1.3× / 1.7× | 1.2× / 1.2× | 2.2× / 2.0× |

Parsing gained 1.3–1.56× (one-shot and build). Counting gained 2.6–3.6×, and the
Python loops 1.67–1.79×. Text sweeps gained most on wikt (`scan_*` 1.80×,
`scan_table` 2.7×), where the per-thread zstd context (#104) removed allocator
contention on many small text frames. On cc, `scan_*` gained only 1.24×: there,
converting 629k matched strings into Python objects, serially under the GIL,
limits it.

## Reproduce

```sh
uvx maturin build --release -m crates/htmlarc-py/Cargo.toml -o target/bench-main-wheel
uv venv -p 3.12 bench-venv
uv pip install -p bench-venv/bin/python target/bench-main-wheel/htmlarc-*.whl \
    lxml==6.1.3 cssselect==1.5.0 beautifulsoup4==4.15.0 soupsieve==2.10 \
    pyarrow==25.0.1 warcio==1.8.1 libzim psutil
export HTMLARC_BENCH_DATA="$PWD/target/release-0.1.1-benchmark-data"
export HTMLARC_CORPUS="$HTMLARC_BENCH_DATA"    # holds a copy of cc_000.warc.gz:
cp corpus/cc_000.warc.gz "$HTMLARC_CORPUS/"    # cold runs rewrite it to evict it
bench-venv/bin/python benchmarks/python-compare/extract.py wikt
bench-venv/bin/python benchmarks/python-compare/extract.py cc
bench-venv/bin/python benchmarks/python-compare/run_suite.py main-d31e8c8.json
```

`run_suite.py` runs every phase in its own process for 3 repeats, warm phases
first and then the cold ones. Cold mode works on macOS only (`F_NOCACHE`).
