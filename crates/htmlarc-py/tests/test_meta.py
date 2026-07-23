"""Typed per-document metadata columns (ADR 0009): build, read back, Arrow export."""

import pytest

import htmlarc

SCHEMA = {"url": str, "status": int, "score": float, "ok": bool}


def build(tmp_path, name="meta.htmlarc"):
    out = tmp_path / name
    with htmlarc.ArchiveBuilder(out, meta_schema=SCHEMA) as b:
        b.add(
            "a",
            "<p>a</p>",
            meta={"url": "https://a", "status": 200, "score": 0.5, "ok": True},
        )
        b.add("b", "<p>b</p>", meta={"url": "https://b", "status": 404})
        b.add("c", "<p>c</p>")  # no meta: all-null row
    return htmlarc.open(out)


def test_meta_schema_round_trips(tmp_path):
    arc = build(tmp_path)
    assert arc.meta_schema == SCHEMA


def test_doc_meta_dict(tmp_path):
    arc = build(tmp_path)
    assert arc["a"].meta == {"url": "https://a", "status": 200, "score": 0.5, "ok": True}
    assert arc["b"].meta == {"url": "https://b", "status": 404, "score": None, "ok": None}
    assert arc["c"].meta == {"url": None, "status": None, "score": None, "ok": None}
    # Typed, not stringly: bool stays bool, int stays int.
    assert arc["a"].meta["ok"] is True
    assert isinstance(arc["a"].meta["status"], int)


def test_no_meta_archive(tmp_path):
    out = tmp_path / "plain.htmlarc"
    with htmlarc.ArchiveBuilder(out) as b:
        b.add("k", "<p>x</p>")
    arc = htmlarc.open(out)
    assert arc.meta_schema is None
    assert arc["k"].meta is None
    with pytest.raises(ValueError, match="no metadata"):
        arc.meta_table()
    with pytest.raises(ValueError, match="no metadata"):
        arc.scan_table("p", meta=["url"])


def test_parsed_document_has_no_meta():
    assert htmlarc.parse("<p>x</p>").meta is None


def test_meta_table_arrow(tmp_path):
    pl = pytest.importorskip("polars")
    arc = build(tmp_path)
    df = pl.DataFrame(arc.meta_table())
    assert df.columns == ["key", "url", "status", "score", "ok"]
    assert df.schema["status"] == pl.Int64
    assert df.schema["score"] == pl.Float64
    assert df.schema["ok"] == pl.Boolean
    assert df["key"].to_list() == ["a", "b", "c"]
    assert df["status"].to_list() == [200, 404, None]
    assert df["ok"].to_list() == [True, None, None]


def test_scan_table_meta_columns(tmp_path):
    pl = pytest.importorskip("polars")
    arc = build(tmp_path)
    df = pl.DataFrame(arc.scan_table("p", text=True, meta=["status", "url"]))
    assert df.columns == ["key", "text", "status", "url"]
    assert df.schema["status"] == pl.Int64
    assert df["status"].to_list() == [200, 404, None]
    assert df["url"].to_list() == ["https://a", "https://b", None]


def test_scan_table_meta_validation(tmp_path):
    arc = build(tmp_path)
    with pytest.raises(ValueError, match="not in the archive's meta_schema"):
        arc.scan_table("p", meta=["nope"])
    with pytest.raises(ValueError, match="collides"):
        arc.scan_table("p", meta=["url", "url"])


def test_schema_validation(tmp_path):
    with pytest.raises(TypeError, match="str, int, float, bool"):
        htmlarc.ArchiveBuilder(meta_schema={"x": list})
    with pytest.raises(ValueError, match="collides with the key column"):
        htmlarc.ArchiveBuilder(meta_schema={"key": str})
    with pytest.raises(ValueError, match="at least one field"):
        htmlarc.ArchiveBuilder(meta_schema={})


def test_row_validation(tmp_path):
    b = htmlarc.ArchiveBuilder(meta_schema={"n": int, "s": str})
    with pytest.raises(ValueError, match="not in the archive's meta_schema"):
        b.add("k", "<p>x</p>", meta={"unknown": 1})
    with pytest.raises(TypeError, match="declared int, got str"):
        b.add("k", "<p>x</p>", meta={"n": "5"})
    with pytest.raises(TypeError, match="declared int, got bool"):
        b.add("k", "<p>x</p>", meta={"n": True})
    with pytest.raises(ValueError, match="without a meta_schema"):
        htmlarc.ArchiveBuilder().add("k", "<p>x</p>", meta={"n": 1})
    # A failed add must not consume the key.
    b.add("k", "<p>x</p>", meta={"n": 5, "s": "ok"})


def test_int_coerces_into_float_field(tmp_path):
    out = tmp_path / "coerce.htmlarc"
    with htmlarc.ArchiveBuilder(out, meta_schema={"score": float}) as b:
        b.add("k", "<p>x</p>", meta={"score": 3})
    assert htmlarc.open(out)["k"].meta == {"score": 3.0}


def test_add_document_carries_meta(tmp_path):
    out = tmp_path / "adddoc.htmlarc"
    doc = htmlarc.parse("<p>hello</p>")
    with htmlarc.ArchiveBuilder(out, meta_schema={"url": str}) as b:
        b.add_document("k", doc, meta={"url": "https://k"})
    assert htmlarc.open(out)["k"].meta == {"url": "https://k"}
