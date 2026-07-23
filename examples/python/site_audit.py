# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "polars"]
# ///
"""Recipe 3 — SEO site audit: crawl once, ask questions forever.

Politely crawls books.toscrape.com (a site built for scraping tutorials),
stores every page in an archive, then:

  1. builds the classic per-page audit table (title / h1 / breadcrumb / prices)
  2. runs audit checks (missing h1s, duplicate titles)
  3. the reveal — a question invented AFTER the crawl (`img:not([alt])`),
     answered from the archive with no re-crawl. Screaming Frog-style tools
     need extraction rules declared before crawling; the archive doesn't.

htmlarc itself is the crawler's parser here: `doc.select_attr("a", "href")`
finds the next links, so the recipe has no bs4/lxml dependency — and
`add_document` stores the already-parsed page, so each page is parsed once.
"""

import sys
import time
from urllib.parse import urldefrag, urljoin, urlparse

import htmlarc
import polars as pl

from _common import DATA, decode_html, get

START = "https://books.toscrape.com/"
ARCHIVE = DATA / "books.htmlarc"
DELAY_S = 0.15


def crawl(limit: int) -> None:
    DATA.mkdir(exist_ok=True)
    builder = htmlarc.ArchiveBuilder()
    queue, seen = [START], {START}
    host = urlparse(START).netloc
    n = 0
    while queue and n < limit:
        url = queue.pop(0)
        r = get(url)
        doc = htmlarc.parse(decode_html(r.content, r.headers.get("Content-Type")))
        builder.add_document(urlparse(url).path or "/", doc)
        n += 1
        for href in doc.select_attr("a[href]", "href"):
            nxt = urldefrag(urljoin(url, href)).url
            if urlparse(nxt).netloc == host and nxt not in seen:
                seen.add(nxt)
                queue.append(nxt)
        time.sleep(DELAY_S)
    builder.write(ARCHIVE)
    print(f"crawled {n} pages -> {ARCHIVE.name}")


def first_text(arc: htmlarc.Archive, selector: str) -> dict[str, str]:
    return {key: txts[0].strip() for key, txts in arc.scan_text(selector) if txts}


if __name__ == "__main__":
    limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else 60
    if not ARCHIVE.exists():
        crawl(limit)

    arc = htmlarc.open(ARCHIVE)

    # 1. the audit table: one scan per field, joined on key
    fields = {
        "title": first_text(arc, "head > title"),
        "h1": first_text(arc, "h1"),
        "crumb": first_text(arc, "ul.breadcrumb li.active"),
        "price": first_text(arc, "p.price_color"),
    }
    audit = pl.DataFrame(
        {"key": arc.keys()}
        | {name: [vals.get(k) for k in arc.keys()] for name, vals in fields.items()}
    )
    print(audit.head(8))

    # 2. audit checks
    print(f"pages missing h1: {audit.filter(pl.col('h1').is_null()).height}")
    dups = audit.drop_nulls("title").group_by("title").len().filter(pl.col("len") > 1)
    print(f"duplicate titles: {dups.height}")

    # 3. two weeks later, NEW questions - answered from the archive, no re-crawl:
    t0 = time.perf_counter()
    one_star = arc.scan_count("p.star-rating.One")
    pages = arc.matching("p.star-rating.One")
    n_noalt = arc.scan_count("img:not([alt])")
    ms = (time.perf_counter() - t0) * 1e3
    print(f"\nnew questions, {ms:.1f} ms after thinking of them:")
    print(f"  {one_star} one-star products, on {len(pages)} pages, e.g. {pages[:3]}")
    print(f"  {n_noalt} images missing alt text (this site passes; your crawl may not)")
