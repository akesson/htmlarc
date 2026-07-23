# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio", "polars", "trafilatura"]
# ///
"""Recipe 7 — the LLM-pipeline loop: re-extract without re-reading the crawl.

Dataset builders re-run main-content extraction over the same crawl every time
their heuristics change (FineWeb ablations, DCLM extractor comparisons, ...).
This recipe runs two extractors over the archive from recipe 1:

  A. trafilatura, fed via `doc.to_html()` — your existing extractor keeps
     working; the archive replaces "download + gunzip + WARC-scan" as the source.
     (trafilatura still re-parses the HTML it's handed — that's its cost.)
  B. a native selector heuristic (`article p, main p` → all `p`) — stays inside
     the pre-parsed DOM, so there is no parse at all.

Honest framing: A saves the I/O and storage of the raw crawl; B additionally
saves the parse. Iterating on B-style structural heuristics is where the
archive turns each experiment from hours into seconds.
"""

import time

import htmlarc
import trafilatura

import warc_to_archive
from _common import DATA

ARCHIVE = DATA / "cc_sample.htmlarc"

if not ARCHIVE.exists():
    warc_to_archive.build()
arc = htmlarc.open(ARCHIVE)
docs = list(arc)  # hold handles: repeated text reads hit warm per-doc caches

t0 = time.perf_counter()
a_chars = a_ok = 0
for doc in docs:
    text = trafilatura.extract(doc.to_html())
    if text:
        a_ok += 1
        a_chars += len(text)
a_secs = time.perf_counter() - t0

t0 = time.perf_counter()
b_chars = b_ok = 0
for doc in docs:
    paras = doc.select_text("article p, main p") or doc.select_text("p")
    text = "\n".join(p for p in paras if len(p.strip()) > 40)
    if text:
        b_ok += 1
        b_chars += len(text)
b_secs = time.perf_counter() - t0

n = len(docs)
print(f"corpus: {n} docs (archive mmap-opened, no crawl re-read)")
print(f"A trafilatura via to_html : {a_secs:6.2f}s  {a_ok}/{n} docs, {a_chars / 1e3:,.0f} k chars")
print(f"B native selector heuristic: {b_secs:6.2f}s  {b_ok}/{n} docs, {b_chars / 1e3:,.0f} k chars")
print(f"heuristic iteration speedup: {a_secs / max(b_secs, 1e-9):.0f}x per experiment")
