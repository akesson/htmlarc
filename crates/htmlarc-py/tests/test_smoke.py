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


def test_select_count(doc):
    assert doc.select_count("p") == 2
    assert doc.select_count(".no-such-class") == 0
    # attr= counts only elements where the attribute is present.
    assert doc.select_count("h1", attr="data-rank") == 1
    assert doc.select_count("p", attr="data-rank") == 0
    assert doc.select_count("div", attr="class") == 1  # class resolves like get()
    assert doc.select_count(htmlarc.Selector("h1.title")) == 1

    # Element-scoped counting only sees the subtree.
    div = doc.select_first("#main")
    assert div.select_count("p") == 2
    assert doc.root.select_count("p") == 2


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

    # scan_count returns one archive-wide total; attr= counts only attribute holders.
    assert archive.scan_count("h1.t") == len(hits)
    assert archive.scan_count("a", attr="href") == len(hits)
    assert archive.scan_count("h1.t", attr="href") == 0
    assert archive.scan_count(".absent") == 0
    assert archive.scan_count(htmlarc.Selector("h1.t, a")) == 2 * len(hits)


@pytest.fixture
def scan_archive(tmp_path):
    """The scan corpus (20 docs, every 3rd carries `h1.t` + `a href`), opened."""
    path = tmp_path / "scan.htmlarc"
    builder = htmlarc.ArchiveBuilder()
    for i in range(20):
        if i % 3 == 0:
            html = f"<body><h1 class='t'>Doc {i}</h1><a href='/l{i}'>x</a></body>"
        else:
            html = f"<body><p>filler {i}</p></body>"
        builder.add(f"page-{i:02}", html)
    builder.write(path)
    return htmlarc.open(path)


def test_scan_table_attr(scan_archive):
    """scan_table flattens to one row per match; attr column matches scan_attr exactly."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    t = pa.table(archive.scan_table("a", attrs=["href"]))
    assert t.column_names == ["key", "href"]
    got = list(zip(t["key"].to_pylist(), t["href"].to_pylist()))
    # scan_attr returns (key, [values]) per doc; flatten to (key, value) rows.
    expected = [(k, v) for k, vals in archive.scan_attr("a", "href") for v in vals]
    assert got == expected
    # One row per matched element, in archive order.
    assert len(t) == archive.scan_count("a")


def test_scan_table_text(scan_archive):
    """text=True yields a text column equal to a flattened scan_text."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    t = pa.table(archive.scan_table("h1.t", text=True))
    assert t.column_names == ["key", "text"]
    got = list(zip(t["key"].to_pylist(), t["text"].to_pylist()))
    expected = [(k, v) for k, vals in archive.scan_text("h1.t") for v in vals]
    assert got == expected


def test_scan_table_combined_nulls_and_class(scan_archive):
    """Combined text + attrs: absent attr -> None, class synthesized like get()."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    t = pa.table(archive.scan_table("h1.t", text=True, attrs=["href", "class"]))
    assert t.column_names == ["key", "text", "href", "class"]
    # h1.t elements have no href -> all null; class is the synthesized "t".
    assert t["href"].to_pylist() == [None] * len(t)
    assert t["class"].to_pylist() == ["t"] * len(t)
    assert t["text"].to_pylist() == [f"Doc {int(k[5:])}" for k in t["key"].to_pylist()]


def test_scan_table_empty_string_vs_null(tmp_path):
    """A present-but-empty attribute is "" (not null); a missing one is null."""
    pa = pytest.importorskip("pyarrow")
    path = tmp_path / "empty.htmlarc"
    builder = htmlarc.ArchiveBuilder()
    builder.add("d0", "<body><a href=''>x</a><a>y</a></body>")
    builder.write(path)
    archive = htmlarc.open(path)

    t = pa.table(archive.scan_table("a", attrs=["href"]))
    assert t["href"].to_pylist() == ["", None]  # empty string, then absent


def test_scan_table_empty_result_keeps_schema(scan_archive):
    """No matches still yields a correctly-typed, zero-row table with every column."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    t = pa.table(archive.scan_table(".absent", text=True, attrs=["href"]))
    assert t.column_names == ["key", "text", "href"]
    assert len(t) == 0


def test_scan_table_key_only(scan_archive):
    """text=False, attrs=None -> a one-column inventory; row count == scan_count."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    r = archive.scan_table("h1.t")
    assert len(r) == archive.scan_count("h1.t")
    t = pa.table(r)
    assert t.column_names == ["key"]


def test_scan_table_reconsumable(scan_archive):
    """The result exports a fresh stream each time and outlives its own handle."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    r = archive.scan_table("a", attrs=["href"])
    first = pa.table(r)
    second = pa.table(r)
    assert first.equals(second)
    # The table holds its own buffers, so it stays valid after the result is dropped.
    del r
    assert first["href"].to_pylist() == [f"/l{i}" for i in range(20) if i % 3 == 0]


def test_scan_table_column_collisions(scan_archive):
    """Column-name collisions are rejected before the sweep runs."""
    archive = scan_archive
    with pytest.raises(ValueError):
        archive.scan_table("a", attrs=["key"])
    with pytest.raises(ValueError):
        archive.scan_table("a", text=True, attrs=["text"])
    with pytest.raises(ValueError):
        archive.scan_table("a", attrs=["href", "HREF"])  # case-insensitive duplicate
    # But "text" as an attribute is fine when there is no text column.
    archive.scan_table("a", attrs=["text"])


def test_scan_table_multi_batch(scan_archive, monkeypatch):
    """A tiny per-batch byte budget forces the result to split into several RecordBatches;
    the flattened table must be byte-identical to the single-batch result (this is the only
    exercise of the >1-doc-per-batch cut path — real corpora fit in one batch)."""
    pa = pytest.importorskip("pyarrow")
    archive = scan_archive

    baseline = pa.table(archive.scan_table("a", text=True, attrs=["href"]))

    # 1 byte forces a cut at every document boundary -> one batch per matching document.
    monkeypatch.setenv("HTMLARC_SCAN_TABLE_BATCH_BYTES", "1")
    r = archive.scan_table("a", text=True, attrs=["href"])
    split = pa.table(r)

    assert split.equals(baseline)
    # Confirm it really split: reading the stream yields more than one batch.
    reader = pa.RecordBatchReader.from_stream(r)
    assert reader.read_all().num_rows == baseline.num_rows
    n_batches = sum(1 for _ in pa.RecordBatchReader.from_stream(archive.scan_table("a")))
    assert n_batches > 1


def test_scan_table_polars(scan_archive):
    """polars consumes the same stream (optional; skipped when polars is absent)."""
    pl = pytest.importorskip("polars")
    archive = scan_archive

    df = pl.DataFrame(archive.scan_table("a", attrs=["href"]))
    assert df.columns == ["key", "href"]
    assert df["href"].to_list() == [f"/l{i}" for i in range(20) if i % 3 == 0]


def test_filter(tmp_path):
    """Filter combines css/key includes and excludes; matching() accepts it."""
    path = tmp_path / "filter.htmlarc"
    builder = htmlarc.ArchiveBuilder()
    for i in range(10):
        table = "<table class='data'><tr><td>x</td></tr></table>" if i % 2 == 0 else ""
        builder.add(f"k,{i}", f"<body><h1>Doc {i}</h1>{table}</body>")  # commas in keys
    builder.write(path)
    archive = htmlarc.open(path)
    evens = [f"k,{i}" for i in range(10) if i % 2 == 0]

    assert archive.matching(htmlarc.Filter()) == archive.keys()  # empty filter keeps all
    assert archive.matching(htmlarc.Filter(include_css="table.data")) == evens
    assert archive.matching(htmlarc.Filter(exclude_css="table.data")) == [
        f"k,{i}" for i in range(10) if i % 2 == 1
    ]
    # Multiple selectors AND; a comma inside one selector string is OR.
    assert archive.matching(htmlarc.Filter(include_css=["h1", "table.data"])) == evens
    assert archive.matching(htmlarc.Filter(include_css="h1, table.data")) == archive.keys()

    # Keys containing commas work (the CLI rule syntax can't express these).
    f = htmlarc.Filter(include_keys=["k,1", "k,4", "absent"], exclude_keys=["k,1"])
    assert archive.matching(f) == ["k,4"]
    assert archive.matching(
        htmlarc.Filter(include_css="table.data", exclude_keys=["k,0"])
    ) == evens[1:]

    assert "table.data" in repr(htmlarc.Filter(include_css="table.data"))
    with pytest.raises(ValueError):
        htmlarc.Filter(include_css="!!!")


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
