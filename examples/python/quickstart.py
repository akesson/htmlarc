# /// script
# requires-python = ">=3.10"
# dependencies = ["htmlarc", "polars"]
# ///
"""5-minute quickstart: every core htmlarc idea on generated data.

No downloads, no corpus — the "crawl" is 30 small pages generated below.
Each numbered section works as a standalone notebook cell.

Run:  uv run --with ../../target/wheels/htmlarc-*.whl quickstart.py
(plain `uv run quickstart.py` once htmlarc is on PyPI)
"""
import htmlarc
import polars as pl

# ---- 0. A tiny fake corpus: 30 blog posts as raw HTML strings ---------------
AUTHORS = ["ada", "brian", "carol"]
pages = {
    f"post-{i}": f"""
    <html><body>
      <article>
        <h1 class="title">Post {i}: notes on topic {i % 5}</h1>
        <p class="byline">by {AUTHORS[i % 3]}</p>
        <p>Body text for post {i}. <a href="/post-{(i + 1) % 30}">next</a>
           {'<a href="https://example.com/ref">reference</a>' if i % 2 else ''}</p>
      </article>
    </body></html>"""
    for i in range(30)
}

# ---- 1. Parse one document (fault-tolerant, html5-style recovery) -----------
doc = htmlarc.parse(pages["post-7"])
el = doc.select_first("h1.title")
print("1.", el.text)

# ---- 2. Build an archive — parse each page ONCE, keep the DOMs --------------
# meta_schema stores typed per-document columns inside the archive: no sidecar
# file to join later.
with htmlarc.ArchiveBuilder("data/quickstart.htmlarc",
                            meta_schema={"author": str, "topic": int}) as b:
    for i, (key, html) in enumerate(pages.items()):
        b.add(key, html, meta={"author": AUTHORS[i % 3], "topic": i % 5})
print("2. wrote data/quickstart.htmlarc")

# ---- 3. Open and query — no HTML parsing happens at read time ---------------
arc = htmlarc.open("data/quickstart.htmlarc")
print("3.", len(arc), "docs;", arc["post-3"].select_first("h1").text)

# ---- 4. Sweep the whole archive: all cores, GIL released --------------------
titles = dict(arc.scan_text("h1.title"))            # {key: [texts]}
hrefs = arc.scan_attr("a[href]", "href")            # per-doc attribute values
n_links = arc.scan_count("a[href]", attr="href")    # just a number: fastest
print("4.", len(titles), "titles,", n_links, "links")

# ---- 5. The dataframe question: one flat Arrow table from one sweep ---------
# One row per matched element; metadata columns ride along on every row.
links = pl.DataFrame(arc.scan_table("a[href]", attrs=["href"],
                                    meta=["author", "topic"]))
by_author = links.group_by("author").len().sort("len", descending=True)
print("5.", by_author)

# ---- 6. Ask a NEW question of the same archive — milliseconds, no re-crawl --
# The point of htmlarc: the extraction schema is NOT fixed at crawl time.
external = arc.scan_count("a[href^='https://']")
print("6.", external, "external links (a question invented after the 'crawl')")
