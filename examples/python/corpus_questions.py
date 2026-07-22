# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio", "polars"]
# ///
"""Recipe 2 — ask a corpus three questions, each in milliseconds.

Opens the archive built by recipe 1 (building it first if missing) and answers:

  Q1  where do these pages link? (top external domains, joined with the sidecar)
  Q2  how many pages ship JSON-LD structured data, and of which @type?
  Q3  how many headings / links are there in total? (counted in Rust, no strings)

The point: none of these questions re-parses any HTML, and the *next* question
you think of tomorrow won't either.
"""

import json
import time
from collections import Counter

import htmlarc
import polars as pl

import warc_to_archive
from _common import DATA

ARCHIVE = DATA / "cc_sample.htmlarc"
JSONLD = 'script[type="application/ld+json"]'


def timed(label: str, fn):
    t0 = time.perf_counter()
    out = fn()
    print(f"  [{(time.perf_counter() - t0) * 1e3:6.1f} ms] {label}")
    return out


if not ARCHIVE.exists():
    warc_to_archive.build()

arc = timed("open archive (mmap)", lambda: htmlarc.open(ARCHIVE))
meta = pl.read_parquet(DATA / "cc_sample_meta.parquet")

print("\nQ1: top external link targets")
links = timed("scan_table('a[href]')", lambda: pl.DataFrame(arc.scan_table("a[href]", attrs=["href"])))
top = (
    links.join(meta, on="key")  # sidecar join: every row knows its source url + fetch date
    .with_columns(domain=pl.col("href").str.extract(r"^https?://([^/]+)", 1))
    .drop_nulls("domain")
    .group_by("domain")
    .len()
    .sort("len", descending=True)
    .head(10)
)
print(top)

print("\nQ2: JSON-LD structured data")
with_ld = timed(f"matching({JSONLD!r})", lambda: arc.matching(JSONLD))
print(f"  {len(with_ld)}/{len(arc)} pages carry JSON-LD ({100 * len(with_ld) / len(arc):.0f}%)")
types: Counter[str] = Counter()
for _key, blocks in arc.scan_text(JSONLD):
    for block in blocks:
        try:
            data = json.loads(block)
        except json.JSONDecodeError:
            continue
        for node in data if isinstance(data, list) else [data]:
            t = isinstance(node, dict) and node.get("@type")
            for name in t if isinstance(t, list) else [t] if t else []:
                types[str(name)] += 1
print(f"  top @type: {types.most_common(8)}")

print("\nQ3: corpus totals (counted in Rust, GIL released)")
heads = timed("scan_count('h1, h2, h3')", lambda: arc.scan_count("h1, h2, h3"))
hrefs = timed("scan_count('a[href]')", lambda: arc.scan_count("a[href]", attr="href"))
print(f"  {heads} headings, {hrefs} links across {len(arc)} pages")
