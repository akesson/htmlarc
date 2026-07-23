"""ArchiveBuilder ergonomics: context manager, on_error="skip", skip_missing."""

import pytest

import htmlarc

# ~70k attribute-bearing elements exceeds the 65,535-entry per-document
# attribute arena — the smallest reliable way to trip the capacity limit.
OVERSIZED = "<html><body>" + "<i x=1></i>" * 70_000 + "</body></html>"
SMALL = "<html><body><h1>hi</h1><a href='/a'>a</a><a name='x'>b</a></body></html>"


def test_context_manager_writes_on_clean_exit(tmp_path):
    out = tmp_path / "cm.htmlarc"
    with htmlarc.ArchiveBuilder(out) as b:
        b.add("k", SMALL)
    arc = htmlarc.open(out)
    assert arc.keys() == ["k"]


def test_context_manager_skips_write_on_exception(tmp_path):
    out = tmp_path / "cm_err.htmlarc"
    with pytest.raises(RuntimeError, match="boom"):
        with htmlarc.ArchiveBuilder(out) as b:
            b.add("k", SMALL)
            raise RuntimeError("boom")
    assert not out.exists()


def test_context_manager_requires_path():
    with pytest.raises(ValueError, match="needs a path"):
        with htmlarc.ArchiveBuilder():
            pass


def test_write_uses_constructor_path(tmp_path):
    out = tmp_path / "explicit.htmlarc"
    b = htmlarc.ArchiveBuilder(out)
    b.add("k", SMALL)
    b.write()
    assert htmlarc.open(out).keys() == ["k"]


def test_write_argument_overrides_constructor_path(tmp_path):
    b = htmlarc.ArchiveBuilder(tmp_path / "ignored.htmlarc")
    b.add("k", SMALL)
    b.write(tmp_path / "actual.htmlarc")
    assert (tmp_path / "actual.htmlarc").exists()
    assert not (tmp_path / "ignored.htmlarc").exists()


def test_write_without_any_path_raises():
    b = htmlarc.ArchiveBuilder()
    b.add("k", SMALL)
    with pytest.raises(ValueError, match="no path"):
        b.write()


def test_add_raises_on_capacity_by_default():
    b = htmlarc.ArchiveBuilder()
    with pytest.raises(ValueError, match="per-document capacity"):
        b.add("big", OVERSIZED)


def test_on_error_skip_records_key_and_continues(tmp_path):
    out = tmp_path / "skip.htmlarc"
    with htmlarc.ArchiveBuilder(out, on_error="skip") as b:
        b.add("ok1", SMALL)
        b.add("big", OVERSIZED)
        b.add("ok2", SMALL)
        assert b.skipped == ["big"]
    arc = htmlarc.open(out)
    assert arc.keys() == ["ok1", "ok2"]


def test_on_error_rejects_unknown_mode():
    with pytest.raises(ValueError, match="on_error"):
        htmlarc.ArchiveBuilder(on_error="ignore")


def test_skipped_empty_by_default():
    assert htmlarc.ArchiveBuilder().skipped == []


def test_select_attr_skip_missing():
    doc = htmlarc.parse(SMALL)
    assert doc.select_attr("a", "href") == ["/a", None]
    assert doc.select_attr("a", "href", skip_missing=True) == ["/a"]
    body = doc.select_first("body")
    assert body is not None
    assert body.select_attr("a", "href", skip_missing=True) == ["/a"]


def test_scan_attr_skip_missing(tmp_path):
    out = tmp_path / "scan.htmlarc"
    with htmlarc.ArchiveBuilder(out) as b:
        b.add("k", SMALL)
    arc = htmlarc.open(out)
    assert arc.scan_attr("a", "href") == [("k", ["/a", None])]
    assert arc.scan_attr("a", "href", skip_missing=True) == [("k", ["/a"])]
