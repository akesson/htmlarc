# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "warcio", "polars"]
# ///
"""Recipe 6 — heading-aware RAG chunks from an archive, boilerplate-free.

Turns the Common Crawl sample archive (recipe 1) into embedding-ready chunks:
select INTO the content (`main`/`article`, falling back to `body`), walk it in
document order, split at h1-h3 boundaries, and skip nav/footer/aside subtrees.

"Select in, don't strip out": the Python API is read-only, so instead of
deleting boilerplate nodes you choose the subtree you want and take text from
block elements inside it. Re-chunking with a new strategy re-reads the archive,
not the internet — chunk-size experiments become cheap.

Output: data/chunks.jsonl  ({key, heading, text} per chunk)
"""

import json
import time

import htmlarc

import warc_to_archive
from _common import DATA

ARCHIVE = DATA / "cc_sample.htmlarc"
SKIP = {"nav", "footer", "aside", "header", "form", "script", "style", "noscript"}
BLOCK = {"p", "li", "pre", "blockquote", "td", "dd"}
HEADINGS = {"h1", "h2", "h3"}
MAX_CHARS = 1200


def clean_text(el: htmlarc.Element) -> str:
    """Text of `el` minus any script/style/etc. nested inside it.

    `el.text` includes ALL descendant text — a paragraph carrying an inline
    `<script>` would leak its JavaScript — so blocks are gathered recursively
    with SKIP subtrees pruned.
    """
    parts = [el.own_text or ""]
    parts += [clean_text(c) for c in el.children if c.tag not in SKIP]
    return " ".join(p for p in parts if p)


def chunk_doc(doc: htmlarc.Document) -> list[dict]:
    root = doc.select_first("main, article, [role='main']") or doc.select_first("body")
    if root is None:
        return []
    chunks, heading, buf = [], "", []

    def flush():
        text = " ".join(buf).strip()
        if len(text) > 80:  # drop crumbs
            for i in range(0, len(text), MAX_CHARS):
                chunks.append({"key": doc.key, "heading": heading, "text": text[i : i + MAX_CHARS]})
        buf.clear()

    def walk(el: htmlarc.Element):
        nonlocal heading
        for child in el.children:
            tag = child.tag
            if tag in SKIP:
                continue
            if tag in HEADINGS:
                flush()
                heading = child.text.strip()
            elif tag in BLOCK:
                if t := clean_text(child).strip():
                    buf.append(t)
            else:
                walk(child)

    walk(root)
    flush()
    return chunks


if __name__ == "__main__":
    if not ARCHIVE.exists():
        warc_to_archive.build()
    arc = htmlarc.open(ARCHIVE)

    t0 = time.perf_counter()
    chunks = [c for doc in arc for c in chunk_doc(doc)]
    secs = time.perf_counter() - t0

    out = DATA / "chunks.jsonl"
    with out.open("w") as f:
        for c in chunks:
            f.write(json.dumps(c, ensure_ascii=False) + "\n")
    print(f"{len(chunks)} chunks from {len(arc)} pages in {secs:.1f}s -> {out.name}")
    for c in chunks[:3]:
        print(f"  [{c['heading'][:40]}] {c['text'][:90]}...")
