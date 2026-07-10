"""End-to-end smoke tests for the htmlarc Python module.

Covers each binding code path once: parse, element queries/navigation, selector
compilation, archive build/open/index/iterate, and error mapping. The heavy
correctness testing lives in the Rust crates; these tests assert the FFI layer
wires them up faithfully.
"""

import pytest

import htmlarc

PAGE = """
<html>
  <body>
    <div id="main" class="content wide">
      <h1 class="title" data-rank="1">Alpha</h1>
      <p>First <b>bold</b> paragraph</p>
      <p>Second paragraph</p>
    </div>
  </body>
</html>
"""


@pytest.fixture
def doc():
    return htmlarc.parse(PAGE)


def test_parse_and_select(doc):
    assert doc.key is None
    titles = doc.select("h1.title")
    assert [el.text for el in titles] == ["Alpha"]
    assert doc.select_first(".no-such-class") is None
    assert [el.tag for el in doc.select("p")] == ["p", "p"]


def test_element_accessors(doc):
    div = doc.select_first("#main")
    assert div.tag == "div"
    assert div.id == "main"
    assert div.classes == ["content", "wide"]
    assert "Alpha" in div.text

    h1 = doc.select_first("h1")
    assert h1.attrs == {"class": "title", "data-rank": "1"}
    assert h1.get("class") == "title"
    assert div.get("class") == "content wide"
    assert h1.get("data-rank") == "1"
    assert h1.get("DATA-RANK") == "1"  # ASCII case-insensitive
    assert h1.get("missing") is None
    assert h1["data-rank"] == "1"
    with pytest.raises(KeyError):
        h1["missing"]
    assert h1.own_text == "Alpha"
    assert h1.css_path == "html > body > div#main.content.wide > h1.title"
    assert "<h1" in h1.to_html() and "Alpha" in h1.to_html()
    assert "h1.title" in repr(h1)


def test_navigation(doc):
    h1 = doc.select_first("h1")
    div = h1.parent
    assert div.id == "main"
    assert [c.tag for c in div.children] == ["h1", "p", "p"]
    assert h1.next_sibling.tag == "p"
    assert h1.prev_sibling is None
    first_p, second_p = doc.select("p")
    assert second_p.prev_sibling.index == first_p.index
    assert doc.root.parent is None
    assert h1.document.key is None


def test_matches_and_compiled_selector(doc):
    h1 = doc.select_first("h1")
    assert h1.matches("h1.title")
    assert not h1.matches("p")
    sel = htmlarc.Selector("h1.title")
    assert sel.source == "h1.title"
    assert [el.text for el in doc.select(sel)] == ["Alpha"]
    assert h1.matches(sel)


def test_document_render(doc):
    raw = doc.to_html()
    assert raw.count("<p>") == 2
    assert doc.to_html(pretty=True) != raw
    assert "Alpha" in doc.text


def test_invalid_selector_raises(doc):
    with pytest.raises(ValueError):
        doc.select("li:nth-of-type(")
    with pytest.raises(ValueError):
        htmlarc.Selector("!!!")


def test_batch_extraction(doc):
    assert doc.select_text("p") == ["First bold paragraph", "Second paragraph"]
    assert doc.select_attr("h1", "data-rank") == ["1"]
    assert doc.select_attr("h1", "class") == ["title"]  # class synthesized, like get()
    assert doc.select_attr("p", "data-rank") == [None, None]
    assert doc.select_html("b") == ["<b>bold</b>"]

    # Element-scoped batch extraction only sees the subtree.
    div = doc.select_first("#main")
    assert div.select_text("h1") == ["Alpha"]
    assert div.select_text(".no-such-class") == []

    # Compiled selectors work in batch calls too.
    assert doc.select_text(htmlarc.Selector("h1.title")) == ["Alpha"]


def test_archive_scan(tmp_path):
    """matching/scan_text/scan_attr sweep the archive in parallel (GIL released)."""
    path = tmp_path / "scan.htmlarc"
    builder = htmlarc.ArchiveBuilder()
    for i in range(20):
        if i % 3 == 0:
            html = f"<body><h1 class='t'>Doc {i}</h1><a href='/l{i}'>x</a></body>"
        else:
            html = f"<body><p>filler {i}</p></body>"
        builder.add(f"page-{i:02}", html)
    builder.write(path)
    archive = htmlarc.open(path)

    hits = [f"page-{i:02}" for i in range(20) if i % 3 == 0]
    assert archive.matching("h1.t") == hits  # archive order, non-matching docs omitted
    assert archive.matching(".absent") == []

    assert archive.scan_text("h1.t") == [(k, [f"Doc {int(k[5:])}"]) for k in hits]
    assert archive.scan_attr("a", "href") == [(k, [f"/l{int(k[5:])}"]) for k in hits]
    # Matched elements without the attribute report None (doc matched, value absent).
    assert archive.scan_attr("h1.t", "href") == [(k, [None]) for k in hits]


def test_archive_roundtrip(tmp_path):
    path = tmp_path / "corpus.htmlarc"
    builder = htmlarc.ArchiveBuilder()
    for i in range(5):
        builder.add(f"page-{i}", f"<html><body><h1 class='t'>Doc {i}</h1></body></html>")
    builder.write(path)
    with pytest.raises(RuntimeError):
        builder.write(path)  # consumed

    archive = htmlarc.open(path)
    assert len(archive) == 5
    assert archive.keys() == [f"page-{i}" for i in range(5)]
    assert "page-3" in archive and "nope" not in archive

    # Index by key, position, and negative position.
    assert archive["page-2"].select_first("h1").text == "Doc 2"
    assert archive[0].key == "page-0"
    assert archive[-1].key == "page-4"
    with pytest.raises(KeyError):
        archive["nope"]
    with pytest.raises(IndexError):
        archive[5]
    assert archive.get("nope") is None
    assert archive.get("page-1").key == "page-1"

    # Iterate with a compiled selector — the scan-shaped access pattern.
    sel = htmlarc.Selector("h1.t")
    texts = [doc.select_first(sel).text for doc in archive]
    assert texts == [f"Doc {i}" for i in range(5)]

    # An OwnedDoc keeps the archive mapping alive after the Archive object dies.
    doc = archive["page-4"]
    del archive
    assert doc.select_first("h1").text == "Doc 4"


def test_open_missing_archive(tmp_path):
    with pytest.raises(IOError):
        htmlarc.open(tmp_path / "missing.htmlarc")
