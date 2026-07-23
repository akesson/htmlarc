"""Enforce that htmlarc.pyi docstrings mirror the pyo3 docstrings in lib.rs.

Editors read only the shipped .pyi stub (never the compiled module), so every
docstring is duplicated: the /// comment in lib.rs is authoritative and the
stub carries a copy for hover. This test fails on any drift between the two,
in either direction — including runtime docs missing from the stub entirely.

Comparison is on normalized text: whitespace collapsed, markup stripped
(Rust's `x`/[`x`] vs the stub's ``x``), and Rust paths (`A::b`) mapped to
Python dots — so reflowing lines is fine, changing words is not.
"""

import ast
import re
from pathlib import Path

import htmlarc

STUB = Path(__file__).resolve().parent.parent / "htmlarc.pyi"

# Members that legitimately have no docstring on either side.
UNDOCUMENTED_OK = {"__init__", "__repr__", "__contains__"}

# CPython replaces C-slot method docs (__len__, __iter__, ...) with these generic
# texts — pyo3 /// comments on slot methods are lost at runtime, so for these the
# stub is the only carrier and is exempt from comparison.
GENERIC_SLOT_DOCS = {
    "Return len(self).",
    "Return selfkey.",  # "Return self[key]." after markup stripping
    "Implement iter(self).",
    "Implement next(self).",
}


def normalize(doc: str | None) -> str:
    if not doc:
        return ""
    doc = doc.replace("::", ".")
    doc = re.sub(r"[`\[\]]", "", doc)
    return re.sub(r"\s+", " ", doc).strip()


def runtime_doc(obj) -> str:
    return normalize(getattr(obj, "__doc__", None))


def stub_members(node: ast.ClassDef) -> dict[str, str]:
    """name -> normalized docstring for every def in a stub class body."""
    return {
        item.name: normalize(ast.get_docstring(item))
        for item in node.body
        if isinstance(item, ast.FunctionDef)
    }


def collect_mismatches() -> list[str]:
    tree = ast.parse(STUB.read_text())
    problems: list[str] = []

    def compare(where: str, stub_doc: str, run_doc: str) -> None:
        if stub_doc != run_doc:
            problems.append(
                f"{where}:\n  stub:    {stub_doc or '(missing)'}\n  runtime: {run_doc or '(missing)'}"
            )

    compare("module htmlarc", normalize(ast.get_docstring(tree)), runtime_doc(htmlarc))

    for node in tree.body:
        if isinstance(node, ast.FunctionDef):
            compare(
                f"htmlarc.{node.name}",
                normalize(ast.get_docstring(node)),
                runtime_doc(getattr(htmlarc, node.name)),
            )
        if not isinstance(node, ast.ClassDef):
            continue
        cls = getattr(htmlarc, node.name)
        compare(f"class {node.name}", normalize(ast.get_docstring(node)), runtime_doc(cls))

        stubbed = stub_members(node)
        for name, stub_doc in stubbed.items():
            if name in UNDOCUMENTED_OK:
                continue
            run_doc = runtime_doc(getattr(cls, name))
            if run_doc in GENERIC_SLOT_DOCS:
                continue
            compare(f"{node.name}.{name}", stub_doc, run_doc)

        # The reverse direction: every documented runtime member must be stubbed.
        for name in dir(cls):
            if name in stubbed or name in UNDOCUMENTED_OK:
                continue
            if name.startswith("_") and name not in {"__getitem__", "__len__", "__iter__", "__next__", "__arrow_c_stream__"}:
                continue
            run_doc = runtime_doc(getattr(cls, name))
            if run_doc and run_doc not in GENERIC_SLOT_DOCS:
                problems.append(f"{node.name}.{name}: documented at runtime, missing from stub")

    return problems


def test_stub_docstrings_mirror_runtime():
    problems = collect_mismatches()
    assert not problems, (
        f"{len(problems)} stub/runtime docstring mismatch(es) "
        "(lib.rs is authoritative - update htmlarc.pyi to match):\n\n"
        + "\n\n".join(problems)
    )
