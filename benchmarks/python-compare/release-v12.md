# Format v12 release measurements

Measured 2026-09-09 using the release build of htmlarc 0.1.0 on Apple M4 Pro,
macOS, CPython 3.12.13, lxml 6.1.3 and cssselect 1.5.0. Each cell is the median
of three separate processes, seconds. Raw timings, environment, input hashes,
counts, failures, and memory peaks are in [release-v12.json](release-v12.json).

| Operation (three selectors) | Wiktionary | Common Crawl |
|---|---:|---:|
| Build archive (parse + write) | 0.5527 | 2.9493 |
| Parse + extract, lxml | 0.6620 | 3.1930 |
| Parse + extract, htmlarc | 0.4748 | 2.4790 |
| Reopen + extract, htmlarc Python loop | 0.0862 | 0.3299 |
| Extract again, htmlarc parallel scan | 0.0364 | 0.0880 |
| Reopen + count, htmlarc Python loop | 0.0448 | 0.1595 |
| Count again, htmlarc parallel scan | 0.0115 | 0.0173 |
| Reparse + count, lxml | 0.5905 | 2.9424 |

Wiktionary: 9,567 documents; v12 archive 68.5 MB. Common Crawl: 5,000 documents;
v12 archive 277.4 MB. Exact UTF-8 input sizes and hashes are in the JSON.

These are warm-filesystem measurements, not cold disk latency. The reopened
Python-loop cases include opening the archive; the parallel scans immediately
follow the loop in the same process and benefit from its warmed pages/text
cache. lxml reparses the source HTML; htmlarc's requery cases require the
one-time archive build shown above. Parallel scans use available cores; the
lxml measurements here are single-process. This table does not claim equivalent
work or universal speedups across these paths.

All three selectors agree on the Wiktionary input: 111,303 links, 43,286 headings,
and 2,172 first table cells. On Common Crawl, lxml gives 614,346 / 34,707 / 34,092
and reports four parse failures; htmlarc gives 629,227 / 35,318 / 37,481 and no
parse failures. Tree recovery differs, so Common Crawl timings are not a
correctness-equivalent comparison. htmlarc's Python loop and parallel scans
agree, and each phase's counts are stable across all three runs.

Reproduce with the existing `extract.py` and `bench.py`; source corpora are
local-only and are described in the root README. `HTMLARC_BENCH_DATA` selects
an isolated data directory so this run does not overwrite older archives:

```sh
export HTMLARC_BENCH_DATA="$PWD/target/release-benchmark-data"
# Populate cc.pkl and wikt.pkl using extract.py (see its module documentation).
# Use an environment with the release-built wheel and versions listed above.
for corpus in wikt cc; do
  for repeat in 1 2 3; do
    for phase in build_htmlarc oneshot_lxml oneshot_htmlarc requery_htmlarc requery_htmlarc_count requery_lxml_count; do
      python benchmarks/python-compare/bench.py "$phase" "$corpus"
    done
  done
done
```

The older README tables remain historical format-v11 measurements. Do not use
their numbers as current release claims.
