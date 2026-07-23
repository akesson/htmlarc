# Type stubs for the htmlarc extension module (shipped in the wheel by maturin).
# Keep in sync with crates/htmlarc-py/src/lib.rs — the pyclass docstrings there
# are the authoritative behavior reference; the docstrings here mirror them so
# editors (which read only this stub, never the compiled module) show them on
# hover.
"""Query pre-parsed HTML document archives (``.htmlarc``) with CSS selectors."""

from collections.abc import Iterator, Sequence
from os import PathLike
from types import TracebackType
from typing import Literal, overload

__version__: str

class Selector:
    """A compiled CSS selector list.

    Compile once and reuse across ``select()`` calls to skip re-parsing the
    selector for every document — the equivalent of ``re.compile`` for CSS.
    """

    def __init__(self, css: str) -> None: ...
    @property
    def source(self) -> str:
        """The selector source text."""

class Filter:
    """An include/exclude predicate over archive documents, for ``Archive.matching()``.

    A document is kept when it satisfies every include condition (or there are
    none) and no exclude condition. Multiple selectors in a list AND together;
    a comma *inside* one selector string is a CSS selector list, i.e. OR. A pure
    key filter (no css) never touches document bodies at all.
    """

    def __init__(
        self,
        *,
        include_css: str | list[str] | None = None,
        include_keys: list[str] | None = None,
        exclude_css: str | list[str] | None = None,
        exclude_keys: list[str] | None = None,
    ) -> None: ...

class Document:
    """A parsed HTML document.

    Obtained from ``htmlarc.parse(html)`` or by indexing an ``Archive``; there
    is no direct constructor. All access goes through elements, starting at
    ``root``.
    """

    @property
    def key(self) -> str | None:
        """The archive key this document was stored under, or ``None`` for parsed documents."""

    @property
    def root(self) -> Element:
        """The document root element (renders the whole document, selects over all of it)."""

    @property
    def text(self) -> str:
        """All descendant text of the document, concatenated."""

    def select(self, selector: str | Selector) -> list[Element]:
        """All elements matching the CSS selector (a string or a compiled
        ``Selector``), in document order."""

    def select_first(self, selector: str | Selector) -> Element | None:
        """The first element matching the CSS selector, or ``None``."""

    def select_text(self, selector: str | Selector) -> list[str]:
        """The descendant text of every element matching the selector, one string
        per match, in document order. One FFI call instead of a Python loop
        over ``select()``."""

    @overload
    def select_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[True]
    ) -> list[str]: ...
    @overload
    def select_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[False] = False
    ) -> list[str | None]:
        """The named attribute of every element matching the selector (``None``
        where absent), in document order. ``"class"`` resolves like
        ``Element.get()``. With ``skip_missing=True``, elements lacking the
        attribute are dropped instead and the result is ``list[str]`` — for
        selectors like ``a[href]`` that guarantee its presence."""

    def select_count(self, selector: str | Selector, attr: str | None = None) -> int:
        """The number of elements matching the selector; with ``attr``, only
        elements where that attribute is present (``"class"`` resolves like
        ``Element.get()``). Counts without materializing elements or text."""

    def select_html(self, selector: str | Selector, pretty: bool = False) -> list[str]:
        """The rendered subtree of every element matching the selector, in
        document order."""

    def to_html(self, pretty: bool = False) -> str:
        """Render the document as HTML. ``pretty=True`` indents; the default is
        the raw compact form."""

class Element:
    """An element within a ``Document``.

    A lightweight handle (document reference + node index); creating and
    dropping elements is cheap and never copies the document.
    """

    @property
    def document(self) -> Document:
        """The document this element belongs to."""

    @property
    def index(self) -> int:
        """The element's node index within the document (stable for the document's lifetime)."""

    @property
    def tag(self) -> str:
        """The tag name, lowercase (e.g. ``"div"``)."""

    @property
    def id(self) -> str | None:
        """The ``id`` attribute, or ``None``."""

    @property
    def classes(self) -> list[str]:
        """The class list, in document order."""

    @property
    def attrs(self) -> dict[str, str]:
        """All attributes as a dict (entity-decoded values). htmlarc stores the
        class list out-of-band, so a ``"class"`` entry is synthesized from it
        (space-joined)."""

    @property
    def text(self) -> str:
        """All descendant text, concatenated (like BeautifulSoup's ``get_text()``).
        Text inside nested ``<script>``/``<style>`` is excluded — unless this
        element itself is the script/style, whose payload is returned."""

    @property
    def own_text(self) -> str | None:
        """The element's own immediate text — its direct text-node children
        concatenated, excluding descendants' text — or ``None`` when it has none."""

    @property
    def css_path(self) -> str:
        """A CSS path locating this element (e.g. ``"html > body > div#main > p"``)."""

    @property
    def parent(self) -> Element | None:
        """The parent element, or ``None`` at the root."""

    @property
    def next_sibling(self) -> Element | None:
        """The next sibling element, or ``None``."""

    @property
    def prev_sibling(self) -> Element | None:
        """The previous sibling element, or ``None``."""

    @property
    def children(self) -> list[Element]:
        """The child elements, in document order."""

    def get(self, name: str) -> str | None:
        """The value of the named attribute (ASCII case-insensitive), or ``None``.
        ``"class"`` resolves through the class list, like ``attrs``."""

    def __getitem__(self, name: str) -> str:
        """``element["href"]`` — like ``get()``, but raises ``KeyError`` when absent."""

    def select(self, selector: str | Selector) -> list[Element]:
        """All descendant elements matching the CSS selector (a string or a
        compiled ``Selector``), in document order."""

    def select_first(self, selector: str | Selector) -> Element | None:
        """The first descendant element matching the CSS selector, or ``None``."""

    def select_text(self, selector: str | Selector) -> list[str]:
        """The descendant text of every matching descendant element, one string
        per match, in document order."""

    @overload
    def select_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[True]
    ) -> list[str]: ...
    @overload
    def select_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[False] = False
    ) -> list[str | None]:
        """The named attribute of every matching descendant element (``None``
        where absent), in document order. ``"class"`` resolves like ``get()``.
        With ``skip_missing=True``, elements lacking the attribute are dropped
        instead and the result is ``list[str]``."""

    def select_count(self, selector: str | Selector, attr: str | None = None) -> int:
        """The number of matching descendant elements; with ``attr``, only
        elements where that attribute is present (``"class"`` resolves like
        ``get()``). Counts without materializing elements or text."""

    def select_html(self, selector: str | Selector, pretty: bool = False) -> list[str]:
        """The rendered subtree of every matching descendant element, in
        document order."""

    def matches(self, selector: str | Selector) -> bool:
        """Whether this element itself matches the CSS selector."""

    def to_html(self, pretty: bool = False) -> str:
        """Render this element's subtree as HTML. ``pretty=True`` indents; the
        default is the raw compact form."""

class Archive:
    """A read-only, memory-mapped ``.htmlarc`` archive.

    Documents are stored pre-parsed: indexing returns a queryable ``Document``
    with no HTML parsing at read time. Index by position (``archive[0]``) or
    key (``archive["…"]``), or iterate to visit every document. The
    ``scan_*``/``matching`` sweeps run across all cores with the GIL released —
    prefer them over a Python loop when extracting from every document.
    """

    def __init__(self, path: str | PathLike[str]) -> None: ...
    @property
    def path(self) -> str:
        """The archive file path."""

    def __len__(self) -> int:
        """The number of documents."""

    def keys(self) -> list[str]:
        """All document keys, in archive order."""

    def __contains__(self, key: str) -> bool: ...
    def __getitem__(self, index: int | str) -> Document:
        """The document at a position (int, negative indexes from the end) or
        under a key (str). Raises ``IndexError`` / ``KeyError`` when absent."""

    def get(self, key: str) -> Document | None:
        """The document under ``key``, or ``None`` when absent."""

    def __iter__(self) -> Iterator[Document]:
        """Iterate over every document, in archive order."""

    def matching(self, selector: str | Selector | Filter) -> list[str]:
        """The keys of every document matching the predicate, in archive order.
        Accepts a CSS selector (string or compiled ``Selector``: keep documents
        with at least one match, swept across all cores) or a ``Filter``
        (include/exclude rules; a pure key filter never touches document
        bodies). Runs with the GIL released either way."""

    def scan_text(self, selector: str | Selector) -> list[tuple[str, list[str]]]:
        """``(key, texts)`` for every document with at least one match: the
        descendant text of each matching element, like ``Document.select_text``
        over the whole archive. Runs across all cores with the GIL released;
        documents without matches are omitted."""

    @overload
    def scan_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[True]
    ) -> list[tuple[str, list[str]]]: ...
    @overload
    def scan_attr(
        self, selector: str | Selector, name: str, *, skip_missing: Literal[False] = False
    ) -> list[tuple[str, list[str | None]]]:
        """``(key, values)`` for every document with at least one match: the
        named attribute of each matching element (``None`` where absent), like
        ``Document.select_attr`` over the whole archive. Runs across all cores
        with the GIL released; documents without matches are omitted. With
        ``skip_missing=True``, elements lacking the attribute are dropped
        instead and the value lists are ``list[str]``."""

    def scan_count(self, selector: str | Selector, attr: str | None = None) -> int:
        """The total number of matching elements across every document; with
        ``attr``, only elements where that attribute is present, like
        ``Document.select_count`` summed over the whole archive. Runs across
        all cores with the GIL released and returns a single int — nothing is
        marshalled per match, and counting never touches document text, so it
        stays on the select-only fast path."""

    def scan_table(
        self,
        selector: str | Selector,
        *,
        text: bool = False,
        attrs: Sequence[str] | None = None,
    ) -> ArrowResult:
        """Every match across the archive as one flat Arrow table: a ``key``
        column (the document key, repeated once per matched element), an
        optional ``text`` column (each element's text content, when
        ``text=True``), and one nullable column per name in ``attrs`` (the
        attribute value, ``null`` where the matched element lacks it;
        ``"class"`` is synthesized space-joined like ``Element.get``). One row
        per matched element, ordered by document then match — the same ordering
        as ``scan_text``/``scan_attr``. With ``text=False`` and no ``attrs``,
        it returns a one-column inventory of which document each match came
        from.

        The sweep runs across all cores with the GIL released and assembles the
        Arrow buffers entirely off-GIL, handing them to Python zero-copy over
        the Arrow PyCapsule stream interface (``pyarrow.table(r)``,
        ``polars.DataFrame(r)``, duckdb, ...). No per-match Python object is
        created, so this is far faster than ``scan_text``/``scan_attr`` when
        extracting from every document. Raises ``ValueError`` if a requested
        attribute name collides with the ``key``/``text`` columns or duplicates
        another (names are matched case-insensitively)."""

class ArrowResult:
    """A columnar scan result (``Archive.scan_table``), exported zero-copy over
    the Arrow PyCapsule *stream* interface. Consume it with
    ``pyarrow.table(r)``, ``polars.DataFrame(r)``,
    ``pandas.DataFrame.from_arrow(r)``, ``duckdb.sql("... from r")``, or any
    other ``__arrow_c_stream__`` reader — htmlarc itself carries no
    Python-side Arrow dependency. The result is re-consumable: each call
    exports a fresh stream over the same buffers.
    """

    def __arrow_c_stream__(self, requested_schema: object | None = None) -> object:
        """Export the table as an Arrow C stream (a PyCapsule named
        ``"arrow_array_stream"``). The ``requested_schema`` hint is accepted
        and ignored — the schema is fixed by the scan, which the PyCapsule
        spec permits."""

    def __len__(self) -> int:
        """The total number of rows (matched elements) across all batches."""

class ArchiveIter:
    """Iterator over an ``Archive``'s documents."""

    def __iter__(self) -> ArchiveIter: ...
    def __next__(self) -> Document: ...

class ArchiveBuilder:
    """Builds a ``.htmlarc`` archive from HTML strings or parsed documents.

    Add documents with ``add(key, html)`` or ``add_document(key, doc)``
    (duplicate keys are skipped, first wins — matching the archive's dedup
    rule), then ``write(path)`` once. The builder cannot be reused after
    writing.

    With a ``path`` at construction the builder is a context manager: ``with
    htmlarc.ArchiveBuilder("out.htmlarc") as b:`` writes on clean exit and
    skips the write when the block raises.

    ``on_error="skip"`` records the key of any document that exceeds htmlarc's
    per-document capacity in ``skipped`` instead of raising — the mode for
    wild-corpus ingestion, where roughly 1 in 100k real-world pages trips a
    capacity limit.
    """

    def __init__(
        self,
        path: str | PathLike[str] | None = None,
        *,
        on_error: Literal["raise", "skip"] = "raise",
    ) -> None: ...
    def __enter__(self) -> ArchiveBuilder:
        """Enter the context manager. Requires a ``path`` from the constructor —
        the write destination for ``__exit__``."""

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> bool:
        """Exit the context manager: write the archive to the constructor's
        ``path`` on clean exit; skip the write when the block raised."""

    @property
    def skipped(self) -> list[str]:
        """Keys of documents dropped by ``on_error="skip"``, in add order. Empty
        when ``on_error="raise"`` (the default) or when nothing was dropped."""

    def add(self, key: str, html: str) -> None:
        """Parse ``html`` and add it under ``key``. Raises ``ValueError`` when
        the HTML exceeds htmlarc's per-document capacity — unless the builder
        was created with ``on_error="skip"``, in which case the document is
        dropped and its key appended to ``skipped``."""

    def add_document(self, key: str, doc: Document) -> None:
        """Add an already-parsed ``Document`` under ``key`` without re-parsing —
        the path for crawlers that parse each page anyway (e.g. for link
        discovery): parse once, query for links, then store the same ``Document``.

        Accepts documents from ``parse()``. Documents handed out by an
        ``Archive`` are backed by shared per-bundle storage and can't be
        re-added directly; raises ``TypeError`` for those (round-trip through
        ``add(key, doc.to_html())`` instead)."""

    def write(self, path: str | PathLike[str] | None = None) -> None:
        """Write the archive and consume the builder. ``path`` may be omitted
        when it was given at construction; passing one here overrides the
        constructor's."""

def parse(html: str) -> Document:
    """Parse an HTML string into a queryable ``Document``.

    Parsing is fault-tolerant in the html5-recovery sense: malformed markup
    yields a best-effort tree rather than an error. Raises ``ValueError`` only
    when the document exceeds htmlarc's per-document capacity."""

def open(path: str | PathLike[str]) -> Archive:
    """Open a ``.htmlarc`` archive (alias for ``Archive(path)``)."""
