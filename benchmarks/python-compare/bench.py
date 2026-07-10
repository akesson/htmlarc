"""One benchmark phase per process: `bench.py <phase> <corpus>` prints one JSON line.

Phases:
  oneshot_bs4 | oneshot_lxml | oneshot_htmlarc  parse + 3-selector extract, streaming
  build_htmlarc                                  ArchiveBuilder over all docs -> .htmlarc
  requery_htmlarc                                open archive + 3-selector extract
                                                 (python loop AND parallel scan_*)
  hot_bs4 | hot_lxml                             parse all -> hold trees -> query hot
Corpora: wikt | cc  (pickles of list[(key, html)] made by extract.py, in data/)
"""

import json
import pickle
import resource
import sys
import time
from pathlib import Path

DIR = Path(__file__).resolve().parent / "data"

Q_LINKS = "a[href]"
Q_HEADS = "h1, h2, h3"
Q_CELLS = "table tr td:first-child"


def load(corpus):
    with open(DIR / f"{corpus}.pkl", "rb") as f:
        return pickle.load(f)


def rss_mb():
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1e6  # bytes on macOS


def emit(phase, corpus, secs, counts, failures=0, **extra):
    print(json.dumps({"phase": phase, "corpus": corpus, "secs": secs,
                      "rss_mb": round(rss_mb(), 1), "counts": counts,
                      "failures": failures, **extra}))


# ---------------------------------------------------------------- bs4

def bs4_query(soup, counts):
    for el in soup.select(Q_LINKS):
        if el.get("href") is not None:
            counts[0] += 1
    for el in soup.select(Q_HEADS):
        el.get_text()
        counts[1] += 1
    for el in soup.select(Q_CELLS):
        el.get_text()
        counts[2] += 1


def oneshot_bs4(corpus):
    from bs4 import BeautifulSoup

    docs = load(corpus)
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, html in docs:
        try:
            soup = BeautifulSoup(html, "lxml")
        except Exception:
            failures += 1
            continue
        bs4_query(soup, counts)
    emit("oneshot_bs4", corpus, {"total": time.perf_counter() - t0}, counts, failures)


def hot_bs4(corpus):
    from bs4 import BeautifulSoup

    docs = load(corpus)
    t0 = time.perf_counter()
    trees, failures = [], 0
    for _key, html in docs:
        try:
            trees.append(BeautifulSoup(html, "lxml"))
        except Exception:
            failures += 1
    t1 = time.perf_counter()
    counts = [0, 0, 0]
    for soup in trees:
        bs4_query(soup, counts)
    t2 = time.perf_counter()
    emit("hot_bs4", corpus, {"parse": t1 - t0, "query": t2 - t1}, counts, failures)


# ---------------------------------------------------------------- lxml

def lxml_selectors():
    from lxml.cssselect import CSSSelector

    return CSSSelector(Q_LINKS), CSSSelector(Q_HEADS), CSSSelector(Q_CELLS)


def lxml_query(tree, sels, counts):
    s_links, s_heads, s_cells = sels
    for el in s_links(tree):
        if el.get("href") is not None:
            counts[0] += 1
    for el in s_heads(tree):
        el.text_content()
        counts[1] += 1
    for el in s_cells(tree):
        el.text_content()
        counts[2] += 1


def oneshot_lxml(corpus):
    import lxml.html

    # lxml's best-practice input is bytes: feeding a str raises ValueError on any
    # doc with an XML encoding declaration (66/5000 on cc). Encode outside the timer.
    docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    sels = lxml_selectors()
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, html in docs:
        try:
            tree = lxml.html.fromstring(html)
        except Exception:
            failures += 1
            continue
        lxml_query(tree, sels, counts)
    emit("oneshot_lxml", corpus, {"total": time.perf_counter() - t0}, counts, failures)


def hot_lxml(corpus):
    import lxml.html

    docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    sels = lxml_selectors()
    t0 = time.perf_counter()
    trees, failures = [], 0
    for _key, html in docs:
        try:
            trees.append(lxml.html.fromstring(html))
        except Exception:
            failures += 1
    t1 = time.perf_counter()
    counts = [0, 0, 0]
    for tree in trees:
        lxml_query(tree, sels, counts)
    t2 = time.perf_counter()
    emit("hot_lxml", corpus, {"parse": t1 - t0, "query": t2 - t1}, counts, failures)


# ---------------------------------------------------------------- htmlarc

def htmlarc_selectors():
    import htmlarc

    return htmlarc.Selector(Q_LINKS), htmlarc.Selector(Q_HEADS), htmlarc.Selector(Q_CELLS)


def htmlarc_query(doc, sels, counts):
    s_links, s_heads, s_cells = sels
    counts[0] += sum(1 for h in doc.select_attr(s_links, "href") if h is not None)
    counts[1] += len(doc.select_text(s_heads))
    counts[2] += len(doc.select_text(s_cells))


def oneshot_htmlarc(corpus):
    import htmlarc

    docs = load(corpus)
    sels = htmlarc_selectors()
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, html in docs:
        try:
            doc = htmlarc.parse(html)
        except Exception:
            failures += 1
            continue
        htmlarc_query(doc, sels, counts)
    emit("oneshot_htmlarc", corpus, {"total": time.perf_counter() - t0}, counts, failures)


def build_htmlarc(corpus):
    import os

    import htmlarc

    docs = load(corpus)
    path = DIR / f"{corpus}.htmlarc"
    failures = 0
    t0 = time.perf_counter()
    b = htmlarc.ArchiveBuilder()
    for key, html in docs:
        try:
            b.add(key, html)
        except Exception:
            failures += 1
    t1 = time.perf_counter()
    b.write(path)
    t2 = time.perf_counter()
    emit("build_htmlarc", corpus, {"parse_add": t1 - t0, "write": t2 - t1},
         [0, 0, 0], failures, archive_mb=round(os.path.getsize(path) / 1e6, 1))


def requery_htmlarc(corpus):
    import htmlarc

    sels = htmlarc_selectors()
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    counts = [0, 0, 0]
    for doc in arc:
        htmlarc_query(doc, sels, counts)
    t2 = time.perf_counter()
    # Same extraction via the GIL-released parallel sweeps.
    s_links, s_heads, s_cells = sels
    scan_counts = [0, 0, 0]
    for _k, hrefs in arc.scan_attr(s_links, "href"):
        scan_counts[0] += sum(1 for h in hrefs if h is not None)
    for _k, texts in arc.scan_text(s_heads):
        scan_counts[1] += len(texts)
    for _k, texts in arc.scan_text(s_cells):
        scan_counts[2] += len(texts)
    t3 = time.perf_counter()
    emit("requery_htmlarc", corpus, {"open": t1 - t0, "loop_query": t2 - t1,
                                     "scan_query": t3 - t2},
         counts, scan_counts=scan_counts, n_docs=len(arc))


if __name__ == "__main__":
    globals()[sys.argv[1]](sys.argv[2])
