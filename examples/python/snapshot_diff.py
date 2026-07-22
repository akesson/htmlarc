# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio", "polars"]
# ///
"""Recipe 4 — diff two snapshots of the same site, a year apart.

Pulls the same domain's pages from two Common Crawl crawls via the CDX index
(single-record range requests — no bulk download), builds one archive per
snapshot (the archive-per-batch pattern for anything that arrives over time),
and diffs per-page titles with a polars join: added / removed / changed.

The same shape answers monitoring questions ("what changed since the selector
was patched?") and migration audits ("did the re-platform lose canonicals?").
"""

import io
import json
import sys
import time

import htmlarc
import polars as pl
from warcio.archiveiterator import ArchiveIterator

from _common import DATA, decode_html, get

CC_DATA = "https://data.commoncrawl.org"
DOMAIN = "docs.python.org/3/library/*"


def crawl_ids() -> tuple[str, str]:
    ids = [c["id"] for c in get("https://index.commoncrawl.org/collinfo.json").json()]
    return ids[min(9, len(ids) - 1)], ids[0]  # ~a year apart: older, newest


def snapshot(crawl_id: str, limit: int) -> htmlarc.Archive:
    path = DATA / f"snap_{crawl_id}.htmlarc"
    if path.exists():
        return htmlarc.open(path)
    rows = get(
        f"https://index.commoncrawl.org/{crawl_id}-index",
        params={
            "url": DOMAIN,
            "output": "json",
            "filter": "status:200",
            "collapse": "urlkey",
            "limit": str(limit),
        },
    ).text
    builder, n = htmlarc.ArchiveBuilder(), 0
    for line in rows.splitlines():
        rec = json.loads(line)
        if "html" not in rec.get("mime", ""):
            continue
        start = int(rec["offset"])
        r = get(
            f"{CC_DATA}/{rec['filename']}",
            headers={"Range": f"bytes={start}-{start + int(rec['length']) - 1}"},
        )
        warc = next(ArchiveIterator(io.BytesIO(r.content)))
        body = warc.content_stream().read()
        if body:
            ct = warc.http_headers.get_header("Content-Type") if warc.http_headers else None
            builder.add(rec["url"], decode_html(body, ct))  # key = URL, same in both snapshots
            n += 1
        time.sleep(0.1)
    builder.write(path)
    print(f"{crawl_id}: {n} pages -> {path.name}")
    return htmlarc.open(path)


def titles(arc: htmlarc.Archive, name: str) -> pl.DataFrame:
    rows = [(k, t[0].strip()) for k, t in arc.scan_text("head > title") if t]
    return pl.DataFrame(rows, schema=["key", name], orient="row")


if __name__ == "__main__":
    limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else 100
    DATA.mkdir(exist_ok=True)
    old_id, new_id = crawl_ids()
    print(f"comparing {old_id} vs {new_id} for {DOMAIN}")
    old, new = snapshot(old_id, limit), snapshot(new_id, limit)

    diff = titles(old, "old").join(titles(new, "new"), on="key", how="full", coalesce=True)
    added = diff.filter(pl.col("old").is_null())
    removed = diff.filter(pl.col("new").is_null())
    changed = diff.drop_nulls().filter(pl.col("old") != pl.col("new"))
    print(f"pages: +{added.height} added, -{removed.height} gone, ~{changed.height} retitled")
    if changed.height:
        print(changed.head(5))
