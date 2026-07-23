"""In-place append (htmlarc.append): round-trip, dedup, meta continuation, recovery."""

import pytest

import htmlarc


def base(tmp_path, meta=False):
    out = tmp_path / "base.htmlarc"
    kwargs = {"meta_schema": {"url": str, "status": int}} if meta else {}
    with htmlarc.ArchiveBuilder(out, **kwargs) as b:
        if meta:
            b.add("old", "<p>old</p>", meta={"url": "https://old", "status": 200})
        else:
            b.add("old", "<p>old</p>")
    return out


def test_append_round_trip(tmp_path):
    out = base(tmp_path)
    with htmlarc.append(out) as a:
        a.add("new", "<p>new</p>")
    arc = htmlarc.open(out)
    assert arc.keys() == ["old", "new"]
    assert arc["new"].select_first("p").text == "new"
    assert arc["old"].select_first("p").text == "old"


def test_append_skips_existing_keys(tmp_path):
    out = base(tmp_path)
    with htmlarc.append(out) as a:
        a.add("old", "<p>SHOULD BE DROPPED</p>")
        a.add("new", "<p>kept</p>")
    arc = htmlarc.open(out)
    assert len(arc) == 2
    assert arc["old"].select_first("p").text == "old"


def test_append_continues_meta(tmp_path):
    out = base(tmp_path, meta=True)
    with htmlarc.append(out) as a:
        a.add("new", "<p>new</p>", meta={"url": "https://new"})
        a.add("bare", "<p>bare</p>")
    arc = htmlarc.open(out)
    assert arc.meta_schema == {"url": str, "status": int}
    assert arc["old"].meta == {"url": "https://old", "status": 200}
    assert arc["new"].meta == {"url": "https://new", "status": None}
    assert arc["bare"].meta == {"url": None, "status": None}


def test_append_meta_without_schema_raises(tmp_path):
    out = base(tmp_path)
    a = htmlarc.append(out)
    with pytest.raises(ValueError, match="without a meta_schema"):
        a.add("k", "<p>x</p>", meta={"url": "x"})


def test_append_write_rejects_other_path(tmp_path):
    out = base(tmp_path)
    a = htmlarc.append(out)
    a.add("new", "<p>x</p>")
    with pytest.raises(ValueError, match="its own file"):
        a.write(tmp_path / "elsewhere.htmlarc")


def test_abandoned_append_leaves_archive_readable(tmp_path):
    out = base(tmp_path)
    a = htmlarc.append(out)
    a.add("lost", "<p>never committed</p>")
    del a  # dropped without write(): recovery contract kicks in
    arc = htmlarc.open(out)
    assert arc.keys() == ["old"]
    # ... and the next append heals the abandoned tail.
    with htmlarc.append(out) as a:
        a.add("kept", "<p>committed</p>")
    assert htmlarc.open(out).keys() == ["old", "kept"]


def test_append_exception_in_with_block_skips_commit(tmp_path):
    out = base(tmp_path)
    with pytest.raises(RuntimeError, match="boom"):
        with htmlarc.append(out) as a:
            a.add("lost", "<p>x</p>")
            raise RuntimeError("boom")
    assert htmlarc.open(out).keys() == ["old"]


def test_append_on_error_skip(tmp_path):
    out = base(tmp_path)
    oversized = "<html><body>" + "<i x=1></i>" * 70_000 + "</body></html>"
    with htmlarc.append(out, on_error="skip") as a:
        a.add("big", oversized)
        a.add("ok", "<p>fine</p>")
        assert a.skipped == ["big"]
    assert htmlarc.open(out).keys() == ["old", "ok"]


def test_append_missing_file_raises(tmp_path):
    with pytest.raises(IOError):
        htmlarc.append(tmp_path / "nope.htmlarc")
