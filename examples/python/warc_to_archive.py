# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio", "polars"]
# ///
"""Recipe 1 — Common Crawl WARC → .htmlarc archive + metadata sidecar.

Streams the first N text/html responses from the latest Common Crawl's first
WARC file (no full-segment download), decodes the bytes, and builds:

    data/cc_sample.htmlarc       the queryable pre-parsed archive
    data/cc_sample_meta.parquet  per-document metadata sidecar (url, date, status)

The sidecar is the recommended pattern for carrying metadata alongside the DOM:
`scan_table` results join back to it on the `key` column.

Run:  uv run --with <htmlarc wheel or 'htmlarc'> warc_to_archive.py [--limit N]
"""

import gzip
import io
import sys
import time

import htmlarc
import polars as pl
from warcio.archiveiterator import ArchiveIterator

from _common import DATA, decode_html, get

ARCHIVE = DATA / "cc_sample.htmlarc"
META = DATA / "cc_sample_meta.parquet"

CDX_API = "https://index.commoncrawl.org/collinfo.json"
CC_DATA = "https://data.commoncrawl.org"


def latest_crawl() -> str:
    return get(CDX_API).json()[0]["id"]


def first_warc_path(crawl_id: str) -> str:
    paths = get(f"{CC_DATA}/crawl-data/{crawl_id}/warc.paths.gz").content
    return gzip.decompress(paths).decode().splitlines()[0]


def build(limit: int = 500) -> None:
    DATA.mkdir(exist_ok=True)
    crawl = latest_crawl()
    warc_url = f"{CC_DATA}/{first_warc_path(crawl)}"
    print(f"streaming {limit} docs from {crawl}\n  {warc_url}")

    builder = htmlarc.ArchiveBuilder()
    meta: list[dict] = []
    t0 = time.perf_counter()
    with get(warc_url, stream=True) as r:
        for rec in ArchiveIterator(r.raw):
            if rec.rec_type != "response" or rec.http_headers is None:
                continue
            ct = rec.http_headers.get_header("Content-Type")
            if not ct or "text/html" not in ct or rec.http_headers.get_statuscode() != "200":
                continue
            body = rec.content_stream().read()
            if not body:
                continue
            url = rec.rec_headers.get_header("WARC-Target-URI")
            key = f"{url}#{len(meta)}"  # index suffix disambiguates re-fetched URLs
            builder.add(key, decode_html(body, ct))
            meta.append(
                {
                    "key": key,
                    "url": url,
                    "fetched": rec.rec_headers.get_header("WARC-Date"),
                    "status": int(rec.http_headers.get_statuscode()),
                    "bytes": len(body),
                }
            )
            if len(meta) >= limit:
                break

    builder.write(ARCHIVE)
    pl.DataFrame(meta).write_parquet(META)
    secs = time.perf_counter() - t0
    mb = sum(m["bytes"] for m in meta) / 1e6
    print(f"{len(meta)} docs ({mb:.0f} MB html) -> {ARCHIVE.name} "
          f"({ARCHIVE.stat().st_size / 1e6:.0f} MB) + {META.name} in {secs:.0f}s")
    print("next: uv run corpus_questions.py  (answers arrive in milliseconds)")


if __name__ == "__main__":
    limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else 500
    build(limit)
