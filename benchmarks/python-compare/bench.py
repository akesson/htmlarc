"""One benchmark phase per process: `bench.py <phase> <corpus>` prints one JSON line.

Phases:
  oneshot_bs4 | oneshot_lxml | oneshot_htmlarc  parse + 3-selector extract, streaming
  build_htmlarc                                  ArchiveBuilder over all docs -> .htmlarc
  requery_htmlarc                                open archive + 3-selector extract
                                                 (python loop AND parallel scan_*)
  requery_htmlarc_scan                           open + the parallel scan_* sweeps only,
                                                 in a fresh process (no loop warming them)
  requery_lxml_par                               the parallel-lxml counterpart: same
                                                 re-parse+extract fanned out over all
                                                 cores (fork Pool AND ThreadPool)
  requery_htmlarc_count |                        the same 3 questions as pure counts:
  requery_lxml_count |                           htmlarc select_count/scan_count vs
  requery_lxml_count_par                         lxml XPath count() (all counting in
                                                 C/Rust, nothing marshalled per match)
  requery_htmlarc_scan_count                     open + scan_count only, fresh process
  newq_htmlarc | newq_lxml | newq_lxml_par       a question nobody planned for at build
                                                 time: pages with JSON-LD + plain-http
                                                 link count (the Common Crawl notebook's)
  requery_htmlarc_arrow                          the extract sweeps as one Arrow table
                                                 each (scan_table), columns built off-GIL
                                                 and handed to pyarrow zero-copy
  hot_bs4 | hot_lxml                             parse all -> hold trees -> query hot
  pipeline_read | pipeline_lxml |                end-to-end from the source cc warc.gz:
  pipeline_lxml_par | pipeline_bs4               stream+decode (+parse+query), cc only
Corpora: wikt | cc  (pickles of list[(key, html)] made by extract.py, in data/)

HTMLARC_BENCH_COLD=1 evicts the phase's on-disk input (the .htmlarc, or
data/cc.warc.gz for pipeline_*) from the page cache before timing starts; every
htmlarc and pipeline phase reports the input's cached fraction as
`resident_at_start`. Without it, those inputs are read through once first, so
"warm" means fully cached.
Multiprocessing phases also report `tree_rss_mb`: the peak of the summed RSS of
the parent and its workers, sampled every 10 ms by a separate process during a
second, untimed pass (so the sampler's CPU use can't touch the timing, and no
sampler thread is alive when the pool forks). It counts copy-on-write pages
shared after fork once per process, so it overstates physical memory.
"""

import json
import os
import pickle
import resource
import sys
import time
from pathlib import Path

DIR = Path(os.environ.get("HTMLARC_BENCH_DATA", Path(__file__).resolve().parent / "data"))

Q_LINKS = "a[href]"
Q_HEADS = "h1, h2, h3"
Q_CELLS = "table tr td:first-child"


def load(corpus):
    with open(DIR / f"{corpus}.pkl", "rb") as f:
        return pickle.load(f)


def rss_mb():
    scale = 1e6 if sys.platform == "darwin" else 1e3
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / scale


def emit(phase, corpus, secs, counts, failures=0, **extra):
    print(json.dumps({"phase": phase, "corpus": corpus, "secs": secs,
                      "rss_mb": round(rss_mb(), 1), "counts": counts,
                      "failures": failures, **extra}))


# ------------------------------------------------- page cache and memory

def _libc():
    import ctypes
    import mmap

    libc = ctypes.CDLL(None, use_errno=True)
    libc.mmap.restype = ctypes.c_void_p
    libc.mmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_int,
                          ctypes.c_int, ctypes.c_int, ctypes.c_long]
    libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    return ctypes, mmap, libc


def resident_frac(path):
    """Fraction of the file's pages in the page cache (mincore on an untouched map)."""
    ctypes, mmap, libc = _libc()
    size = os.path.getsize(path)
    fd = os.open(path, os.O_RDONLY)
    try:
        addr = libc.mmap(None, size, mmap.PROT_READ, mmap.MAP_SHARED, fd, 0)
        if addr in (None, ctypes.c_void_p(-1).value):
            raise OSError(ctypes.get_errno(), "mmap")
        pages = (size + mmap.PAGESIZE - 1) // mmap.PAGESIZE
        vec = (ctypes.c_ubyte * pages)()
        rc = libc.mincore(addr, size, vec)
        libc.munmap(addr, size)
        if rc != 0:
            raise OSError(ctypes.get_errno(), "mincore")
        return round(sum(b & 1 for b in vec) / pages, 4)
    finally:
        os.close(fd)


def prepare_input(path):
    """Evict `path` from the page cache when HTMLARC_BENCH_COLD=1, without root
    (unlike `purge` or drop_caches). macOS: rewrite it through an F_NOCACHE copy.
    Linux: posix_fadvise(DONTNEED). Returns the cached fraction left at the start
    of the timed region. Warm mode reads the file through once instead, so a
    long-unused input is fully cached again."""
    import fcntl

    if os.environ.get("HTMLARC_BENCH_COLD") != "1":
        with open(path, "rb", buffering=0) as f:
            while f.read(8 << 20):
                pass
    elif hasattr(fcntl, "F_NOCACHE"):
        tmp = Path(f"{path}.evict")
        with open(path, "rb", buffering=0) as src, open(tmp, "wb", buffering=0) as dst:
            fcntl.fcntl(src.fileno(), fcntl.F_NOCACHE, 1)
            fcntl.fcntl(dst.fileno(), fcntl.F_NOCACHE, 1)
            while chunk := src.read(8 << 20):
                dst.write(chunk)
            os.fsync(dst.fileno())
        os.replace(tmp, path)
    elif hasattr(os, "posix_fadvise"):
        fd = os.open(path, os.O_RDONLY)
        try:
            os.fsync(fd)  # DONTNEED skips dirty pages, e.g. a just-built archive
            os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
        finally:
            os.close(fd)
    else:
        raise OSError("cold mode needs F_NOCACHE (macOS) or posix_fadvise (Linux)")
    return resident_frac(path)


# Runs as its own process, so the parent stays single-threaded when its pool forks.
# Samples the parent and all its descendants except itself until stdin closes.
_SAMPLER = """
import os, select, sys
import psutil
me, peak = psutil.Process(int(sys.argv[1])), 0
print("ready", flush=True)
while True:
    total = 0
    for p in [me, *me.children(recursive=True)]:
        if p.pid == os.getpid():
            continue
        try:
            total += p.memory_info().rss
        except psutil.Error:
            pass  # worker exited between listing and reading
    peak = max(peak, total)
    if select.select([sys.stdin], [], [], 0.01)[0]:
        break
print(peak)
"""


class TreeRss:
    """Peak summed RSS of this process and all its children, sampled by a child
    process."""

    def __enter__(self):
        import subprocess

        self._proc = subprocess.Popen([sys.executable, "-c", _SAMPLER, str(os.getpid())],
                                      stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                      text=True)
        if self._proc.stdout.readline().strip() != "ready":
            raise RuntimeError("RSS sampler failed to start (is psutil installed?)")
        return self

    def __exit__(self, *exc):
        out, _ = self._proc.communicate()
        self.peak = int(out)

    @property
    def mb(self):
        return round(self.peak / 1e6, 1)


def timed_then_measured(fn):
    """(seconds, result, tree_rss_mb): fn() timed without a sampler, then run again
    untimed under TreeRss for its memory peak."""
    t0 = time.perf_counter()
    result = fn()
    secs = time.perf_counter() - t0
    with TreeRss() as mem:
        fn()
    return secs, result, mem.mb


def timed(fn):
    """(seconds, result) of one fn() call."""
    t0 = time.perf_counter()
    result = fn()
    return time.perf_counter() - t0, result


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


def par_setup(corpus):
    """Load the corpus as bytes into _par_docs (inherited by fork workers) and split
    it into ~4 index ranges per worker for balance. Returns (workers, chunks)."""
    global _par_docs
    _par_docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    workers = os.cpu_count()
    step = max(1, len(_par_docs) // (workers * 4))
    chunks = [(i, min(i + step, len(_par_docs))) for i in range(0, len(_par_docs), step)]
    return workers, chunks


def mp_fork():
    import multiprocessing as mp

    return mp.get_context("fork")


def fork_sweep(fn, chunks, workers):
    with mp_fork().Pool(workers) as pool:
        return pool.map(fn, chunks)


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


def sum_results(results, n):
    """Totals of a pool's [(counts, failures)] results: (counts, failures)."""
    return [sum(c[i] for c, _ in results) for i in range(n)], sum(f for _, f in results)


def requery_lxml_par(corpus):
    from multiprocessing.pool import ThreadPool

    workers, chunks = par_setup(corpus)

    def thread_sweep():
        with ThreadPool(workers) as pool:
            return pool.map(_lxml_par_chunk, chunks)

    # includes pool startup + result IPC: that's the real per-question cost
    t_proc, results, tree_mb = timed_then_measured(
        lambda: fork_sweep(_lxml_par_chunk, chunks, workers))
    counts, failures = sum_results(results, 3)
    # Threads share this process, so there's no tree to measure: time it only.
    t_thread, t_results = timed(thread_sweep)
    assert sum_results(t_results, 3) == (counts, failures)
    emit("requery_lxml_par", corpus, {"processes": t_proc, "threads": t_thread},
         counts, failures, workers=workers, tree_rss_mb=tree_mb)


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
    workers, chunks = par_setup(corpus)
    secs, results, tree_mb = timed_then_measured(
        lambda: fork_sweep(_lxml_count_chunk, chunks, workers))
    counts, failures = sum_results(results, 3)
    emit("requery_lxml_count_par", corpus, {"processes": secs}, counts, failures,
         workers=workers, tree_rss_mb=tree_mb)


# ---------------------------------------------------------------- htmlarc

def htmlarc_selectors():
    import htmlarc

    return htmlarc.Selector(Q_LINKS), htmlarc.Selector(Q_HEADS), htmlarc.Selector(Q_CELLS)


def htmlarc_query(doc, sels, counts):
    s_links, s_heads, s_cells = sels
    counts[0] += sum(1 for h in doc.select_attr(s_links, "href") if h is not None)
    counts[1] += len(doc.select_text(s_heads))
    counts[2] += len(doc.select_text(s_cells))


def htmlarc_scan_counts(arc, sels):
    """The extraction via the GIL-released parallel sweeps; returns match counts."""
    s_links, s_heads, s_cells = sels
    counts = [0, 0, 0]
    for _k, hrefs in arc.scan_attr(s_links, "href"):
        counts[0] += sum(1 for h in hrefs if h is not None)
    for _k, texts in arc.scan_text(s_heads):
        counts[1] += len(texts)
    for _k, texts in arc.scan_text(s_cells):
        counts[2] += len(texts)
    return counts


def htmlarc_scan_count(arc, sels):
    """The three questions as pure counts: one int back per sweep."""
    s_links, s_heads, s_cells = sels
    return [arc.scan_count(s_links, attr="href"), arc.scan_count(s_heads),
            arc.scan_count(s_cells)]


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
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    counts = [0, 0, 0]
    for doc in arc:
        htmlarc_query(doc, sels, counts)
    t2 = time.perf_counter()
    # Same extraction via the GIL-released parallel sweeps.
    scan_counts = htmlarc_scan_counts(arc, sels)
    t3 = time.perf_counter()
    emit("requery_htmlarc", corpus, {"open": t1 - t0, "loop_query": t2 - t1,
                                     "scan_query": t3 - t2},
         counts, scan_counts=scan_counts, n_docs=len(arc), resident_at_start=resident)


def requery_htmlarc_scan(corpus):
    import htmlarc

    sels = htmlarc_selectors()
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    scan_counts = htmlarc_scan_counts(arc, sels)
    t2 = time.perf_counter()
    emit("requery_htmlarc_scan", corpus, {"open": t1 - t0, "scan_query": t2 - t1},
         scan_counts, n_docs=len(arc), resident_at_start=resident)


def requery_htmlarc_count(corpus):
    import htmlarc

    sels = htmlarc_selectors()
    s_links, s_heads, s_cells = sels
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
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
    scan_counts = htmlarc_scan_count(arc, sels)
    t3 = time.perf_counter()
    emit("requery_htmlarc_count", corpus, {"open": t1 - t0, "loop_count": t2 - t1,
                                           "scan_count": t3 - t2},
         counts, scan_counts=scan_counts, n_docs=len(arc), resident_at_start=resident)


def requery_htmlarc_scan_count(corpus):
    import htmlarc

    sels = htmlarc_selectors()
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    counts = htmlarc_scan_count(arc, sels)
    t2 = time.perf_counter()
    emit("requery_htmlarc_scan_count", corpus, {"open": t1 - t0, "scan_count": t2 - t1},
         counts, n_docs=len(arc), resident_at_start=resident)


def requery_htmlarc_arrow(corpus):
    import htmlarc
    import pyarrow as pa

    s_links, s_heads, s_cells = htmlarc_selectors()
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    # Columnar sweep: each scan_table builds contiguous Arrow buffers off-GIL, with no
    # per-match PyString marshalling (the cap on scan_text/scan_attr).
    r_links = arc.scan_table(s_links, attrs=["href"])
    r_heads = arc.scan_table(s_heads, text=True)
    r_cells = arc.scan_table(s_cells, text=True)
    t2 = time.perf_counter()
    # Zero-copy handover into pyarrow — should be ~free next to the scan.
    links = pa.table(r_links)
    heads = pa.table(r_heads)
    cells = pa.table(r_cells)
    scan_counts = [links.num_rows - links["href"].null_count,
                   heads.num_rows, cells.num_rows]
    t3 = time.perf_counter()
    emit("requery_htmlarc_arrow", corpus, {"open": t1 - t0, "scan": t2 - t1,
                                           "to_arrow": t3 - t2},
         [0, 0, 0], scan_counts=scan_counts, n_docs=len(arc), resident_at_start=resident)


# A question nobody planned for when the archive was built — the Common Crawl
# notebook's "question you think of tomorrow": how many pages ship JSON-LD, and
# how many links go out over plain http? counts = [pages with JSON-LD, http links].
Q_JSONLD = 'script[type="application/ld+json"]'
Q_HTTP = "a[href^='http://']"


def newq_htmlarc(corpus):
    import htmlarc

    s_ld, s_http = htmlarc.Selector(Q_JSONLD), htmlarc.Selector(Q_HTTP)
    resident = prepare_input(DIR / f"{corpus}.htmlarc")
    t0 = time.perf_counter()
    arc = htmlarc.open(DIR / f"{corpus}.htmlarc")
    t1 = time.perf_counter()
    counts = [len(arc.matching(s_ld)), arc.scan_count(s_http)]
    t2 = time.perf_counter()
    # The same question on one core, after the sweep (so its pages are warm).
    loop_counts = [0, 0]
    for doc in arc:
        loop_counts[0] += doc.select_first(s_ld) is not None
        loop_counts[1] += doc.select_count(s_http)
    t3 = time.perf_counter()
    assert loop_counts == counts, (loop_counts, counts)
    emit("newq_htmlarc", corpus, {"open": t1 - t0, "scan": t2 - t1, "loop": t3 - t2},
         counts, n_docs=len(arc), resident_at_start=resident)


def lxml_newq_selectors():
    from lxml import etree
    from lxml.cssselect import CSSSelector

    # Both halves evaluated inside libxml2: boolean() for "has any", count() for links.
    return (etree.XPath(f"boolean({CSSSelector(Q_JSONLD).path})"),
            etree.XPath(f"count({CSSSelector(Q_HTTP).path})"))


def _lxml_newq_docs(docs):
    import lxml.html

    has_ld, count_http = lxml_newq_selectors()
    counts, failures = [0, 0], 0
    for _key, html in docs:
        try:
            tree = lxml.html.fromstring(html)
        except Exception:
            failures += 1
            continue
        counts[0] += bool(has_ld(tree))
        counts[1] += int(count_http(tree))
    return counts, failures


def newq_lxml(corpus):
    docs = [(k, h.encode("utf-8")) for k, h in load(corpus)]
    t0 = time.perf_counter()
    counts, failures = _lxml_newq_docs(docs)
    emit("newq_lxml", corpus, {"total": time.perf_counter() - t0}, counts, failures)


def _lxml_newq_chunk(rng):
    return _lxml_newq_docs(_par_docs[rng[0]:rng[1]])


def newq_lxml_par(corpus):
    workers, chunks = par_setup(corpus)
    secs, results, tree_mb = timed_then_measured(
        lambda: fork_sweep(_lxml_newq_chunk, chunks, workers))
    counts, failures = sum_results(results, 2)
    emit("newq_lxml_par", corpus, {"processes": secs}, counts, failures,
         workers=workers, tree_rss_mb=tree_mb)


# ------------------------------------------------- end-to-end source pipeline
# What analyzing the corpus actually costs without a pre-parsed artifact: every
# question re-reads the source .warc.gz (gunzip + warcio record scan), decodes,
# parses, queries. htmlarc's counterpart is requery_htmlarc on the .htmlarc that
# was converted once. cc only (the wikt analog would stream the .zim).
# The input is extract.py's prefix of cc_000.warc.gz: the records a run over the
# full file reads, so only those bytes are warmed or evicted.

CC_WARC = DIR / "cc.warc.gz"


def _cc_bodies(limit=5000):
    """Yield (key, raw html bytes) straight off the warc.gz, like extract.py."""
    from warcio.archiveiterator import ArchiveIterator

    n = 0
    with open(CC_WARC, "rb") as f:
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
    resident = prepare_input(CC_WARC)
    t0 = time.perf_counter()
    n = bytes_ = 0
    for _key, body in _cc_bodies():
        body.decode("utf-8", errors="replace")
        n, bytes_ = n + 1, bytes_ + len(body)
    emit("pipeline_read", corpus, {"total": time.perf_counter() - t0},
         [n, 0, 0], html_mb=round(bytes_ / 1e6, 1), resident_at_start=resident)


def pipeline_lxml(corpus):
    import lxml.html

    assert corpus == "cc"
    sels = lxml_selectors()
    counts, failures = [0, 0, 0], 0
    resident = prepare_input(CC_WARC)
    t0 = time.perf_counter()
    for _key, body in _cc_bodies():
        try:
            tree = lxml.html.fromstring(body)  # raw bytes: lxml's native input
        except Exception:
            failures += 1
            continue
        lxml_query(tree, sels, counts)
    emit("pipeline_lxml", corpus, {"total": time.perf_counter() - t0}, counts, failures,
         resident_at_start=resident)


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

    def sweep():
        with mp_fork().Pool(workers) as pool:
            return list(pool.imap_unordered(_pipeline_chunk, chunks(_cc_bodies())))

    resident = prepare_input(CC_WARC)
    secs, results, tree_mb = timed_then_measured(sweep)
    counts, failures = sum_results(results, 3)
    emit("pipeline_lxml_par", corpus, {"total": secs}, counts, failures,
         workers=workers, tree_rss_mb=tree_mb, resident_at_start=resident)


def pipeline_bs4(corpus):
    from bs4 import BeautifulSoup

    assert corpus == "cc"
    counts, failures = [0, 0, 0], 0
    resident = prepare_input(CC_WARC)
    t0 = time.perf_counter()
    for _key, body in _cc_bodies():
        try:
            soup = BeautifulSoup(body, "lxml")  # raw bytes: bs4 detects charset
        except Exception:
            failures += 1
            continue
        bs4_query(soup, counts)
    emit("pipeline_bs4", corpus, {"total": time.perf_counter() - t0}, counts, failures,
         resident_at_start=resident)


if __name__ == "__main__":
    globals()[sys.argv[1]](sys.argv[2])
