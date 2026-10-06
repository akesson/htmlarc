"""Extract original HTML from the measurement corpora into pickles of
list[(key, html_str)] so every library in the benchmark parses identical input.

  data/wikt.pkl  — all text/html articles from wiktionary_co.zim (via libzim)
  data/cc.pkl    — first N text/html 200-responses from cc_000.warc.gz (via warcio)
  data/cc.warc.gz — the byte prefix of cc_000.warc.gz that holds those N records:
                   the pipeline_* phases read it, and cold runs evict it rather than
                   the shared corpus file, which bench.py never touches

The corpus location defaults to <repo root>/corpus (see the repository README,
"Measurement corpus"); override with HTMLARC_CORPUS=/path/to/corpus. Output goes to
data/ unless HTMLARC_BENCH_DATA names another directory (as bench.py reads it).
"""

import os
import pickle
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORPUS = Path(os.environ.get("HTMLARC_CORPUS", HERE.parent.parent / "corpus"))
OUT = Path(os.environ.get("HTMLARC_BENCH_DATA", HERE / "data"))
CC_LIMIT = 5000


def extract_wikt():
    from libzim.reader import Archive

    zim = Archive(CORPUS / "wiktionary_co.zim")
    docs = []
    for i in range(zim.all_entry_count):
        entry = zim._get_entry_by_id(i)
        if entry.is_redirect:
            continue
        item = entry.get_item()
        if not item.mimetype.startswith("text/html"):
            continue
        html = bytes(item.content).decode("utf-8", errors="replace")
        key = entry.title or entry.path
        docs.append((f"{key}#{i}", html))
    return docs


def extract_cc():
    from warcio.archiveiterator import ArchiveIterator

    docs, end = [], None
    with open(CORPUS / "cc_000.warc.gz", "rb") as f:
        records = ArchiveIterator(f)
        for rec in records:
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
            html = body.decode("utf-8", errors="replace")
            uri = rec.rec_headers.get_header("WARC-Target-URI")
            docs.append((f"{uri}#{len(docs)}", html))
            if len(docs) >= CC_LIMIT:
                # Each record is its own gzip member, so this cut is a valid warc.gz.
                end = records.get_record_offset() + records.get_record_length()
                break
        f.seek(0)
        prefix = f.read(end) if end is not None else f.read()
    (OUT / "cc.warc.gz").write_bytes(prefix)
    return docs


if __name__ == "__main__":
    which = sys.argv[1]
    OUT.mkdir(parents=True, exist_ok=True)
    docs = extract_wikt() if which == "wikt" else extract_cc()
    total = sum(len(h) for _, h in docs)
    with open(OUT / f"{which}.pkl", "wb") as f:
        pickle.dump(docs, f, protocol=5)
    print(f"{which}: {len(docs)} docs, {total / 1e6:.1f} MB html")
