# /// script
# requires-python = ">=3.10"
# dependencies = ["requests", "libzim", "polars"]
# ///
"""Recipe 5 — mine a Wiktionary ZIM into a structured dictionary.

Downloads the (small) Corsican Wiktionary ZIM from the Kiwix mirror, converts
it entry-by-entry into an archive via python-libzim + ArchiveBuilder (the
`htmlarc-convert` CLI does the same in one command), then extracts a
kaikki-style word list — every entry's definitions — with CSS selectors over
the *rendered* HTML. No wikitext template expansion, no Lua engine.

htmlarc even parses the mirror's directory listing to find the latest file.
"""

import re
import time
from pathlib import Path

import htmlarc

from _common import DATA, get

MIRROR = "https://download.kiwix.org/zim/wiktionary/"
PATTERN = re.compile(r"wiktionary_co_all_nopic_(\d{4}-\d{2})\.zim$")
ARCHIVE = DATA / "wikt_co.htmlarc"


def download_zim() -> Path:
    listing = htmlarc.parse(get(MIRROR).text)  # dogfood: parse the listing page
    names = [h for h in listing.select_attr("a[href]", "href") if h and PATTERN.search(h)]
    latest = max(names, key=lambda h: m.group(1) if (m := PATTERN.search(h)) else "")
    target = DATA / latest
    if not target.exists():
        print(f"downloading {latest} ...")
        target.write_bytes(get(MIRROR + latest).content)
    return target


def build() -> None:
    from libzim.reader import Archive as ZimArchive

    zim_path = download_zim()
    zim = ZimArchive(zim_path)
    builder, n = htmlarc.ArchiveBuilder(), 0
    t0 = time.perf_counter()
    for i in range(zim.all_entry_count):
        # python-libzim has no public "iterate all entries" API; indexing by
        # entry id via this private method is the accepted community idiom.
        entry = zim._get_entry_by_id(i)
        if entry.is_redirect:
            continue
        item = entry.get_item()
        if not item.mimetype.startswith("text/html"):
            continue
        builder.add(f"{entry.title or entry.path}#{i}", bytes(item.content).decode("utf-8", "replace"))
        n += 1
    builder.write(ARCHIVE)
    print(f"{n} entries parsed+packed in {time.perf_counter() - t0:.1f}s: "
          f"{zim_path.name} ({zim_path.stat().st_size / 1e6:.0f} MB) -> "
          f"{ARCHIVE.name} ({ARCHIVE.stat().st_size / 1e6:.0f} MB)")


if __name__ == "__main__":
    DATA.mkdir(exist_ok=True)
    if not ARCHIVE.exists():
        build()
    arc = htmlarc.open(ARCHIVE)

    # kaikki-style extraction: word + definitions, straight off the rendered DOM
    t0 = time.perf_counter()
    out, senses = [], 0
    for key, defs in arc.scan_text("ol li"):
        if defs:
            word = key.rsplit("#", 1)[0]
            out.append({"word": word, "definitions": [d.strip()[:120] for d in defs[:3]]})
            senses += len(defs)
    ms = (time.perf_counter() - t0) * 1e3
    print(f"{len(out)} words, {senses} senses extracted in {ms:.0f} ms; e.g.:")
    for row in out[:5]:
        print(f"  {row['word']}: {row['definitions'][0] if row['definitions'] else ''}")

    n_ipa = len(arc.matching("span.API, span.IPA"))
    print(f"entries with IPA pronunciation markup: {n_ipa}")
