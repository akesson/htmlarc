# htmlarc main (`17dafb2`) measurements (format v12)

Measured 2026-10-05 with htmlarc built from `main` at `17dafb2` (a local
`maturin build --release`; the package version string still reads 0.1.1, but the
build includes PRs #100–#108, which are **not on PyPI**). Same stack as the 0.1.1
run: lxml 6.1.3, cssselect 1.5.0, BeautifulSoup 4.15.0 + soupsieve 2.10, pyarrow
25.0.1, warcio 1.8.1, CPython 3.12.13, Apple M4 Pro (10P+4E, 14 logical CPUs),
48 GB, macOS 27.0.1, on mains power.
Each cell is the **median of three separate processes**, in seconds. Every phase
of every workflow in the [v11 README](README.md) was re-measured. Raw runs,
environment, input hashes, counts and failures are in
[main-17dafb2.json](main-17dafb2.json), together with the raw runs of the A/B
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

## What changed since the `d31e8c8` run

The previous run of this suite measured `main` at `d31e8c8`. Since then #105 made
attribute-value matching allocation-free and stopped `scan_count` from building
and sorting per-document keys; #106–#108 changed selector semantics (`*`,
`:root`, `[class…]`) without touching anything these selectors use. Effect on the
headline ratios against 14-core lxml:

- **new question (cc): 23× → 59×.** Its `a[href^='http://']` was the selector that
  allocated on every comparison.
- **count, wikt: 32× → 44×**; cc unchanged at about 53×. In a fresh process the cc
  sweep includes a one-time page-fault cost of about 2 ms out of 7 ms, which #105
  did not touch (section 4).
- Everything else moved within about 8%, the run-to-run noise of this machine.

## 1. One-shot extraction (parse + 3 selectors, each document once, 1 core)

| | wikt | cc | parse failures (cc) |
|---|---:|---:|---:|
| BeautifulSoup (lxml backend) | 7.175 | 28.643 | 0 |
| lxml | 0.617 | 2.981 | 4 |
| **htmlarc** | **0.331** | **1.510** | 0 |
| htmlarc vs lxml | 1.86× | 1.97× | |
| htmlarc vs BeautifulSoup | 22× | 19× | |

If you look at a corpus once, htmlarc is about 1.9× faster than lxml.

## 2. Build (one time: parse + write `.htmlarc`, 1 core)

| | wikt | cc |
|---|---:|---:|
| Build time | 0.424 | 1.976 |
| Build time as a fraction of one lxml pass | 0.69× | 0.66× |

## 3. Requery: the next 3-selector extraction over the whole corpus

htmlarc rows open the archive in a fresh process (opening is included and takes
under 0.2 ms). lxml has no artifact, so it re-parses.

| | cores | wikt | cc | RSS (cc) |
|---|---:|---:|---:|---:|
| lxml, re-parse | 1 | 0.617 | 2.981 | † |
| lxml, re-parse, `ThreadPool` | 14 | 0.687 | 2.341 | † |
| lxml, re-parse, `multiprocessing` | 14 | 0.111 | 0.417 | † |
| **htmlarc, Python loop** (`select_attr`/`select_text`) | 1 | **0.057** | **0.197** | 0.42 GB |
| **htmlarc, `scan_*`** | 14 | **0.020** | **0.072** | 0.39 GB |
| **htmlarc, `scan_table` → pyarrow** | 14 | **0.012** | **0.030** | 0.48 GB |

Ratios, same cores on both sides:

| | wikt | cc |
|---|---:|---:|
| 1 core: htmlarc loop vs lxml | 11× | 15× |
| 14 cores: htmlarc `scan_*` vs lxml `multiprocessing` | 5.5× | 5.8× |
| 14 cores: htmlarc `scan_table` vs lxml `multiprocessing` | 9.0× | 14× |

Mixed core counts, labelled as such: htmlarc on 1 core beats lxml on 14
(2.0× wikt, 2.1× cc), and htmlarc `scan_*` on 14 is 31× (wikt) / 41× (cc)
faster than lxml on 1 core.

## 4. Counting: the same three questions answered as counts

lxml uses XPath `count()` inside libxml2. htmlarc uses `select_count` /
`scan_count` inside Rust. Neither marshals a match into Python.

| | cores | wikt | peak RSS | cc | peak RSS |
|---|---:|---:|---:|---:|---:|
| lxml `count()`, re-parse | 1 | 0.549 | † | 2.748 | † |
| lxml `count()`, re-parse, `multiprocessing` | 14 | 0.104 | 0.57 GB ‡ | 0.379 | 2.32 GB ‡ |
| **htmlarc `select_count` loop** | 1 | **0.0184** | 0.10 GB | **0.0481** | 0.23 GB |
| **htmlarc `scan_count`** | 14 | **0.0024** | 0.10 GB | **0.0071** | 0.23 GB |

| | wikt | cc |
|---|---:|---:|
| **14 vs 14 cores: `scan_count` vs lxml `multiprocessing`** | **44×** | **53×** |
| 1 vs 1 core: `select_count` loop vs lxml | 30× | 57× |
| 14 vs 1 core (mixed, not a fair comparison) | 233× | 388× |

The fair headline is the 14-vs-14 row: **about 50× on cc and 40× on wikt**. The
htmlarc side is a few milliseconds, where a fraction of a millisecond moves the
ratio by 10%, so quote these as "about". The `scan_count` rows are each a fresh
process's first sweep, which pays a one-time page-fault cost (about 2 ms on cc):
the same sweep run second in a process takes 0.0049 s on cc.

## 5. Holding every parsed tree in RAM (the in-process alternative to re-parsing)

| query over pre-parsed corpus | cores | wikt | RSS | cc | RSS |
|---|---:|---:|---:|---:|---:|
| BeautifulSoup trees | 1 | 1.732 | 1.25 GB | 6.683 | 4.90 GB |
| lxml trees | 1 | 0.166 | 0.83 GB | 0.953 | 3.68 GB |
| **htmlarc mmap, loop** | 1 | **0.057** | 0.12 GB | **0.197** | 0.42 GB |
| **htmlarc mmap, `scan_*`** | 14 | **0.020** | 0.12 GB | **0.072** | 0.39 GB |

Same core count (1 vs 1), htmlarc querying the archive is 2.9× (wikt) / 4.8× (cc)
faster than lxml querying its own live trees. The RSS columns include the
harness holding the input corpus in RAM. Net of that (subtracting one-shot lxml's
RSS), lxml's trees cost about 0.66 GB (wikt) / 2.32 GB (cc), against htmlarc's
0.12 / 0.42 GB: about 5.5× less. Building the trees costs 0.42 s / 1.91 s for
lxml and 7.6 s / 35.7 s for BeautifulSoup.

## 6. WARC pipeline: per question, straight from `cc_000.warc.gz` (cc only)

| | cores | time | peak RSS |
|---|---:|---:|---:|
| warcio read + decode only, no parsing (the floor) | 1 | 0.700 | 0.05 GB |
| lxml pipeline | 1 | 3.731 | 0.07 GB |
| lxml pipeline, fork + `imap` | 14 | 0.865 | 1.52 GB ‡ |
| BeautifulSoup pipeline (charset detection) | 1 | 29.329 | 0.10 GB |
| **htmlarc `scan_*` on the converted archive** | 14 | **0.072** | 0.39 GB |
| **htmlarc `scan_count`** | 14 | **0.0071** | 0.23 GB |

Reading and decoding the source alone costs 9.7× htmlarc's whole extraction. 14-core
lxml reaches 0.865 s, close to that serial-read floor, and is 12× slower than
`scan_*` (14 vs 14). The pipeline is CPU-bound: the cold-cache runs (section 9) are
within 1% of these warm ones. Raw-bytes input lets lxml/bs4 honour declared
charsets, so their counts shift by about 1% from the in-RAM tables (lxml
623,428 / 35,040 / 36,726, 3 failures).

## 7. Arrow sweep (`requery_htmlarc_arrow`)

The three extractions as three `scan_table` Arrow tables, handed to pyarrow
zero-copy: **0.012 s (wikt) / 0.030 s (cc)**, 14 cores. That is 1.6× / 2.4× faster
than the `scan_*` list sweeps, because columns are built off-GIL. The pyarrow
handover itself takes either 0.2 ms or 2.3–4.1 ms, depending on the process
(bimodal, as on every earlier run; the cause was not investigated).

## 8. New phase: a question nobody planned at build time

This is the [Common Crawl notebook](../../examples/python/common_crawl.ipynb)'s
"question you think of tomorrow": how many pages ship JSON-LD
(`script[type="application/ld+json"]`), and how many links use plain http
(`a[href^='http://']`). htmlarc uses `matching` + `scan_count`. lxml uses XPath
`boolean()` + `count()` in libxml2.

| cc | cores | time | peak RSS | pages with JSON-LD | http links |
|---|---:|---:|---:|---:|---:|
| lxml, re-parse | 1 | 2.364 | † | 1,099 | 246,971 |
| lxml, re-parse, `multiprocessing` | 14 | 0.360 | 2.32 GB ‡ | 1,099 | 246,971 |
| **htmlarc loop** | 1 | **0.024** | 0.25 GB | 1,101 | 252,040 |
| **htmlarc `matching` + `scan_count`** | 14 | **0.0061** | 0.25 GB | 1,101 | 252,040 |

Comparing equal core counts: **59× (14 vs 14)**, 100× (1 vs 1). On the `d31e8c8`
run this was 23× / 47×: `href^=` used to lowercase both strings on every
comparison. `href` values now compare case-sensitively, as the HTML standard and
lxml do, so 6 `HTTP://` links no longer count (252,046 before). The remaining gap
to lxml's count is tree recovery, not case.
On wikt neither selector matches anything (0 / 0). The cost there is 0.102 s for
14-core lxml and 0.0022 s for htmlarc (46×), but the answer is empty, so cite cc.
The htmlarc loop row runs after the sweep in the same process.

## 9. New phase: cold cache (the archive not in the page cache)

For a cold run, the input is evicted before timing by rewriting it through an
`F_NOCACHE` copy. That needs no root, unlike `purge`. `mincore` then confirms that
0% of the file is resident. lxml's in-RAM rows above have no cold equivalent: their
input is already decoded in memory. The cold-vs-cold comparison is the WARC
pipeline.

| | cores | wikt warm | wikt cold | cc warm | cc cold |
|---|---:|---:|---:|---:|---:|
| htmlarc loop, extract | 1 | 0.057 | 0.118 | 0.197 | 0.452 |
| htmlarc loop, count | 1 | 0.0184 | 0.084 | 0.0481 | 0.244 |
| htmlarc `scan_*`, extract | 14 | 0.020 | 0.041 | 0.072 | 0.149 |
| htmlarc `scan_count` | 14 | 0.0024 | 0.021 | 0.0071 | 0.072 |
| htmlarc `scan_table` | 14 | 0.012 | 0.034 | 0.030 | 0.112 |
| htmlarc new question | 14 | 0.0022 | 0.021 | 0.0061 | 0.075 |
| lxml WARC pipeline | 14 | | | 0.865 | 0.873 |

A cold archive costs 2.0–12× more on the 14-core sweeps and 2.1–5.1× more on the
1-core loops. The largest multiples are on the fastest warm steps (`scan_count`,
the new question), where reading the file dominates. Compared with 14-core lxml:

- on the pipeline (cold vs cold, cc): `scan_*` is 5.9× faster and `scan_count`
  12× faster;
- against lxml's warm, in-RAM re-parse, a cold `scan_count` is still 5.3× faster
  on cc and 4.8× on wikt, and a cold `scan_*` is 2.8× (cc) / 2.7× (wikt) faster.

A cold `scan_count` runs at the speed of a sequential read. Reading the evicted
archive sequentially first and then scanning takes 0.020 s (wikt) / 0.074 s (cc)
in total; the read alone is 0.017 / 0.066 s (about 4 GB/s). A cold `scan_count`
takes 0.021 / 0.072 s, the same within noise. On 0.1.1 the same check gave
0.027 / 0.079 s against a cold `scan_count` of 0.076 / 0.113 s (a 34–49 ms gap),
so a prefetch in htmlarc would no longer save anything measurable.

## Caveats (keep these with any number quoted)

- **Not released.** These numbers are for `main`, not the PyPI 0.1.1 wheel. Until a
  release ships PRs #100–#108, `pip install htmlarc` gives the 0.1.1 numbers in
  the A/B table below.
- **One-shot is not where htmlarc wins most**: 1.86–1.97× over lxml. The large ratios
  are for parse-once, query-many workflows and assume the one-time build in
  section 2.
- **State the core counts.** Ratios that mix 1-core lxml with 14-core htmlarc
  (e.g. 388× on counting) are labelled as mixed and are not fair comparisons.
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
  overstates physical memory. The pipeline row (1.52 GB here, 1.49–1.66 GB on earlier
  runs: the sampler's variance) is the cleanest comparison, because its parent
  does not hold the corpus.
- **Noise.** Each phase ran under a shared CPU lock that waits for the 1-minute
  load average to drop below 3 (it was 1.5–3.0 at each phase start). Most steps
  spread within 10% over their three processes. The widest spreads are the wikt
  new-question sweep (27%: 2.8 ms against 2.2 ms), the hot-lxml query on wikt
  (20%), the 14-process lxml rows (13–18%) and the cc `select_count` loop's
  trailing `scan_count` (15%). Rows untouched by #105–#108 agree with the
  `d31e8c8` run within about 8%.

## Compared with the PyPI 0.1.1 wheel

Interleaved A/B on the same archive, right after the suite: each round runs the
phase with the PyPI wheel, then with the main build, in fresh processes, 3 rounds.
The per-round ratios agree within 10%.

| phase (fresh process) | 0.1.1 | main | speedup |
|---|---:|---:|---:|
| one-shot, cc | 2.387 | 1.517 | 1.57× |
| loop extract, cc | 0.331 | 0.197 | 1.68× |
| `scan_*` extract, wikt | 0.037 | 0.020 | 1.84× |
| `scan_*` extract, cc | 0.083 | 0.068 | 1.23× |
| `scan_table`, wikt | 0.031 | 0.012 | 2.5× |
| `scan_table`, cc | 0.044 | 0.029 | 1.51× |
| loop count, cc | 0.174 | 0.051 | 3.4× |
| `scan_count`, wikt | 0.0119 | 0.0024 | 4.9× |
| `scan_count`, cc | 0.0195 | 0.0071 | 2.8× |
| new question loop, cc | 0.088 | 0.024 | 3.7× |
| new question `scan_count`, cc | 0.0187 | 0.0061 | 3.0× |

Headline ratios against lxml (14 vs 14 unless stated):

| ratio | v11 | 0.1.1 | main `d31e8c8` | main `17dafb2` |
|---|---:|---:|---:|---:|
| one-shot, 1 vs 1, cc / wikt | | 1.26× / 1.35× | 1.96× / 1.90× | 1.97× / 1.86× |
| count, cc / wikt | ~25× / ~10× | 20× / 8.4× | 52× / 32× | 53× / 44× |
| extract `scan_*`, cc / wikt | 4–5× | 4.8× / 3.1× | 6.2× / 5.7× | 5.8× / 5.5× |
| extract `scan_table`, cc / wikt | | 9.1× / 3.8× | 15× / 9.8× | 14× / 9.0× |
| new question, cc | | 19× | 23× | 59× |
| WARC pipeline vs `scan_*`, cc | | 10× | 12× | 12× |
| htmlarc 1 core vs lxml 14, cc / wikt | 1.3× / 1.7× | 1.2× / 1.2× | 2.2× / 2.0× | 2.1× / 2.0× |

Parsing gained about 1.57× (one-shot and build). Counting gained 2.8–4.9×, the
extracting Python loop 1.68× and the new-question loop 3.7×. Text sweeps gained
most on wikt (`scan_*` 1.84×, `scan_table` 2.5×), where the per-thread zstd
context (#104) removed allocator contention on many small text frames. On cc,
`scan_*` gained only 1.23×: there, converting 629k matched strings into Python
objects, serially under the GIL, limits it.

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
bench-venv/bin/python benchmarks/python-compare/run_suite.py main-17dafb2.json
```

`run_suite.py` runs every phase in its own process for 3 repeats, warm phases
first and then the cold ones. Cold mode works on macOS only (`F_NOCACHE`).
