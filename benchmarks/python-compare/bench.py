"""One benchmark phase per process: `bench.py <phase> <corpus>` prints one JSON line.

Phases:
  oneshot_bs4 | oneshot_lxml | oneshot_htmlarc  parse + 3-selector extract, streaming
  build_htmlarc                                  ArchiveBuilder over all docs -> .htmlarc
  requery_htmlarc                                open archive + 3-selector extract
                                                 (python loop AND parallel scan_*)
  requery_lxml_par                               the parallel-lxml counterpart: same
                                                 re-parse+extract fanned out over all
                                                 cores (fork Pool AND ThreadPool)
  requery_htmlarc_count |                        the same 3 questions as pure counts:
  requery_lxml_count |                           htmlarc select_count/scan_count vs
  requery_lxml_count_par                         lxml XPath count() (all counting in
                                                 C/Rust, nothing marshalled per match)
  hot_bs4 | hot_lxml                             parse all -> hold trees -> query hot
  pipeline_read | pipeline_lxml |                end-to-end from the source cc warc.gz:
  pipeline_lxml_par | pipeline_bs4               stream+decode (+parse+query), cc only
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


# Parallel lxml: how lxml users actually scale to big corpora. Trees aren't
# picklable, so parse and extraction must be fused inside each worker and only
# plain data returned. Fork start method: workers inherit the already-encoded
# corpus copy-on-write — zero input IPC, the best case for lxml on this box.
_par_docs = None


def _lxml_par_chunk(rng):
    import lxml.html

    sels = lxml_selectors()  # per chunk: lxml XPath evaluators aren't shareable across threads
    i0, i1 = rng
    counts, failures = [0, 0, 0], 0
    for _key, html in _par_docs[i0:i1]:
        try:
            tree = lxml.html.fromstring(html)
        except Exception:
            failures += 1
            continue
        lxml_query(tree, sels, counts)
    return counts, failures


def requery_lxml_par(corpus):
    import multiprocessing as mp
    import os
    from multiprocessing.pool import ThreadPool

    global _par_docs
    _par_docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    workers = os.cpu_count()
    step = max(1, len(_par_docs) // (workers * 4))  # ~4 chunks/worker for balance
    chunks = [(i, min(i + step, len(_par_docs))) for i in range(0, len(_par_docs), step)]

    def run(pool_cls, **kw):
        t0 = time.perf_counter()
        with pool_cls(workers, **kw) as pool:
            results = pool.map(_lxml_par_chunk, chunks)
        secs = time.perf_counter() - t0
        counts = [sum(c[i] for c, _ in results) for i in range(3)]
        failures = sum(f for _, f in results)
        return secs, counts, failures

    # includes pool startup + result IPC: that's the real per-question cost
    t_proc, counts, failures = run(mp.get_context("fork").Pool)
    t_thread, t_counts, t_failures = run(ThreadPool)
    assert t_counts == counts and t_failures == failures
    emit("requery_lxml_par", corpus, {"processes": t_proc, "threads": t_thread},
         counts, failures, workers=workers)


# Count-only re-query: the same three questions answered as pure counts. This is
# each library's fair fast path — lxml counts inside libxml2 via XPath count(),
# htmlarc counts inside Rust via select_count/scan_count — so no Element handle
# or string ever crosses into Python and the comparison isolates parse+engine
# speed from per-match marshalling.

def lxml_count_selectors():
    from lxml import etree
    from lxml.cssselect import CSSSelector

    # CSSSelector.path is the compiled XPath; count() over it evaluates fully in C.
    return tuple(etree.XPath(f"count({CSSSelector(q).path})")
                 for q in (Q_LINKS, Q_HEADS, Q_CELLS))


def lxml_count_query(tree, cnts, counts):
    for i, cnt in enumerate(cnts):
        counts[i] += int(cnt(tree))


def requery_lxml_count(corpus):
    import lxml.html

    docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    cnts = lxml_count_selectors()
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, html in docs:
        try:
            tree = lxml.html.fromstring(html)
        except Exception:
            failures += 1
            continue
        lxml_count_query(tree, cnts, counts)
    emit("requery_lxml_count", corpus, {"total": time.perf_counter() - t0},
         counts, failures)


def _lxml_count_chunk(rng):
    import lxml.html

    cnts = lxml_count_selectors()  # per chunk: XPath evaluators aren't shareable
    i0, i1 = rng
    counts, failures = [0, 0, 0], 0
    for _key, html in _par_docs[i0:i1]:
        try:
            tree = lxml.html.fromstring(html)
        except Exception:
            failures += 1
            continue
        lxml_count_query(tree, cnts, counts)
    return counts, failures


def requery_lxml_count_par(corpus):
    import multiprocessing as mp
    import os

    global _par_docs
    _par_docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    workers = os.cpu_count()
    step = max(1, len(_par_docs) // (workers * 4))
    chunks = [(i, min(i + step, len(_par_docs))) for i in range(0, len(_par_docs), step)]
    t0 = time.perf_counter()
    with mp.get_context("fork").Pool(workers) as pool:
        results = pool.map(_lxml_count_chunk, chunks)
    secs = time.perf_counter() - t0
    counts = [sum(c[i] for c, _ in results) for i in range(3)]
    emit("requery_lxml_count_par", corpus, {"processes": secs}, counts,
         sum(f for _, f in results), workers=workers)


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


def requery_htmlarc_count(corpus):
    import htmlarc

    s_links, s_heads, s_cells = htmlarc_selectors()
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    counts = [0, 0, 0]
    for doc in arc:
        counts[0] += doc.select_count(s_links, attr="href")
        counts[1] += doc.select_count(s_heads)
        counts[2] += doc.select_count(s_cells)
    t2 = time.perf_counter()
    # Same counts via the GIL-released parallel sweep: one int back per question.
    scan_counts = [arc.scan_count(s_links, attr="href"),
                   arc.scan_count(s_heads),
                   arc.scan_count(s_cells)]
    t3 = time.perf_counter()
    emit("requery_htmlarc_count", corpus, {"open": t1 - t0, "loop_count": t2 - t1,
                                           "scan_count": t3 - t2},
         counts, scan_counts=scan_counts, n_docs=len(arc))


# ------------------------------------------------- end-to-end source pipeline
# What analyzing the corpus actually costs without a pre-parsed artifact: every
# question re-reads the source .warc.gz (gunzip + warcio record scan), decodes,
# parses, queries. htmlarc's counterpart is requery_htmlarc on the .htmlarc that
# was converted once. cc only (the wikt analog would stream the .zim).

def _cc_bodies(limit=5000):
    """Yield (key, raw html bytes) straight off the warc.gz, like extract.py."""
    import os

    from warcio.archiveiterator import ArchiveIterator

    corpus = Path(os.environ.get("HTMLARC_CORPUS", DIR.parent.parent.parent / "corpus"))
    n = 0
    with open(corpus / "cc_000.warc.gz", "rb") as f:
        for rec in ArchiveIterator(f):
            if rec.rec_type != "response":
                continue
            ct = rec.http_headers.get_header("Content-Type") if rec.http_headers else None
            if not ct or "text/html" not in ct:
                continue
            if rec.http_headers.get_statuscode() != "200":
                continue
            body = rec.content_stream().read()
            if not body:
                continue
            yield f"cc#{n}", body
            n += 1
            if n >= limit:
                return


def pipeline_read(corpus):
    assert corpus == "cc"
    t0 = time.perf_counter()
    n = bytes_ = 0
    for _key, body in _cc_bodies():
        body.decode("utf-8", errors="replace")
        n, bytes_ = n + 1, bytes_ + len(body)
    emit("pipeline_read", corpus, {"total": time.perf_counter() - t0},
         [n, 0, 0], html_mb=round(bytes_ / 1e6, 1))


def pipeline_lxml(corpus):
    import lxml.html

    assert corpus == "cc"
    sels = lxml_selectors()
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, body in _cc_bodies():
        try:
            tree = lxml.html.fromstring(body)  # raw bytes: lxml's native input
        except Exception:
            failures += 1
            continue
        lxml_query(tree, sels, counts)
    emit("pipeline_lxml", corpus, {"total": time.perf_counter() - t0}, counts, failures)


def _pipeline_chunk(chunk):
    import lxml.html

    sels = lxml_selectors()
    counts, failures = [0, 0, 0], 0
    for _key, body in chunk:
        try:
            tree = lxml.html.fromstring(body)
        except Exception:
            failures += 1
            continue
        lxml_query(tree, sels, counts)
    return counts, failures


def pipeline_lxml_par(corpus):
    import multiprocessing as mp
    import os

    assert corpus == "cc"
    workers = os.cpu_count()

    def chunks(it, size=64):  # batch bodies to amortize IPC
        buf = []
        for kv in it:
            buf.append(kv)
            if len(buf) == size:
                yield buf
                buf = []
        if buf:
            yield buf

    t0 = time.perf_counter()
    with mp.get_context("fork").Pool(workers) as pool:
        results = list(pool.imap_unordered(_pipeline_chunk, chunks(_cc_bodies())))
    secs = time.perf_counter() - t0
    counts = [sum(c[i] for c, _ in results) for i in range(3)]
    emit("pipeline_lxml_par", corpus, {"total": secs}, counts,
         sum(f for _, f in results), workers=workers)


def pipeline_bs4(corpus):
    from bs4 import BeautifulSoup

    assert corpus == "cc"
    counts, failures = [0, 0, 0], 0
    t0 = time.perf_counter()
    for _key, body in _cc_bodies():
        try:
            soup = BeautifulSoup(body, "lxml")  # raw bytes: bs4 detects charset
        except Exception:
            failures += 1
            continue
        bs4_query(soup, counts)
    emit("pipeline_bs4", corpus, {"total": time.perf_counter() - t0}, counts, failures)


if __name__ == "__main__":
    globals()[sys.argv[1]](sys.argv[2])
