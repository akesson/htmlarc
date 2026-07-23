# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio"]
# ///
"""Recipe 1 — Common Crawl WARC → .htmlarc archive with typed metadata columns.

Streams the first N text/html responses from the latest Common Crawl's first
WARC file (no full-segment download), decodes the bytes, and builds one file:

    data/cc_sample.htmlarc   pre-parsed DOMs + per-document metadata columns
                             (url, fetch date, HTTP status, byte size — typed)

The metadata rides inside the archive: readers get it back as `doc.meta`,
`archive.meta_table()` (an Arrow table), or as extra typed columns on any
`scan_table(...)` result — no sidecar file, no join. `on_error="skip"` drops
the ~1-in-100k page that exceeds a per-document capacity limit instead of
crashing the ingest.

Run:  uv run --with <htmlarc wheel or 'htmlarc'> warc_to_archive.py [--limit N]
"""

import argparse
import gzip
import time

import htmlarc
from warcio.archiveiterator import ArchiveIterator

from _common import DATA, decode_html, get

ARCHIVE = DATA / "cc_sample.htmlarc"
SCHEMA = {"url": str, "fetched": str, "status": int, "bytes": int}

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

    n, total_bytes = 0, 0
    t0 = time.perf_counter()
    with htmlarc.ArchiveBuilder(ARCHIVE, on_error="skip", meta_schema=SCHEMA) as builder:
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
                builder.add(
                    f"{url}#{n}",  # index suffix disambiguates re-fetched URLs
                    decode_html(body, ct),
                    meta={
                        "url": url,
                        "fetched": rec.rec_headers.get_header("WARC-Date"),
                        "status": int(rec.http_headers.get_statuscode()),
                        "bytes": len(body),
                    },
                )
                n += 1
                total_bytes += len(body)
                if n >= limit:
                    break
        skipped = builder.skipped
    secs = time.perf_counter() - t0
    print(f"{n - len(skipped)} docs ({total_bytes / 1e6:.0f} MB html) -> {ARCHIVE.name} "
          f"({ARCHIVE.stat().st_size / 1e6:.0f} MB, metadata columns included) in {secs:.0f}s"
          + (f" ({len(skipped)} oversized doc(s) skipped)" if skipped else ""))
    print("next: uv run corpus_questions.py  (answers arrive in milliseconds)")


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=500, help="documents to stream (default 500)")
    build(ap.parse_args().limit)
