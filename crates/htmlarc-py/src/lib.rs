//! Python bindings for htmlarc.
//!
//! Design (see the wrappability review, PR #45): PyO3 classes must be `'static + Send + Sync`,
//! so every class wraps an *owned* handle — [`DomInner`] for freshly parsed documents,
//! [`OwnedDoc`] for archived ones, [`OwnedSelectorList`] for compiled selectors. A Python
//! [`Element`] is `(Py<Document>, node index)`, mirroring the Rust `HtmlElement = (&dom, index)`
//! shape: the cheap borrowing handle is rebuilt inside each method call, and nothing borrow-tied
//! ever crosses the FFI boundary (iterators are collected to index vectors per call).

use std::path::PathBuf;
use std::sync::Arc;

use htmlarc_archive::{ArchiveErr, HtmlArchiveBuilder, MmapArchive, OwnedDoc};
use htmlarc_dom::prelude::{
    DomInner, DomIterator, HtmlDoc, HtmlElement, HtmlFormat, NodeIndex, OwnedSelectorList,
};
use pyo3::exceptions::{PyIOError, PyIndexError, PyKeyError, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

/// The two owned DOM backings a [`Document`] can hold. Both are `Send + Sync + 'static` and
/// implement `DomRead` with the same `LinearSweep` forward iterator, so every query answers
/// identically regardless of where the document came from.
enum Backing {
    /// Parsed in-process from an HTML string ([`parse`]). Boxed: `DomInner` is ~4× the size
    /// of the other variant, and the extra indirection is invisible next to the FFI call.
    Parsed(Box<DomInner>),
    /// Resolved out of a memory-mapped `.htmlarc` file ([`Archive`]). Holds its `Arc` to the
    /// archive, so it stays valid even after the Python `Archive` object is garbage-collected.
    Archived(OwnedDoc),
}

/// Run `$body` with `$el` bound to the borrowing `HtmlElement` for `$doc`/`$idx`. The two match
/// arms instantiate the same generic code for each backing type — the wrapper-level stand-in for
/// a read-only enum backing in the core. `$body` must return owned data (no `'dom` borrows).
macro_rules! with_el {
    ($doc:expr, $idx:expr, |$el:ident| $body:expr) => {
        match &$doc.backing {
            Backing::Parsed(dom) => {
                let $el = HtmlElement::new(dom.as_ref(), NodeIndex::new($idx));
                $body
            }
            Backing::Archived(dom) => {
                let $el = HtmlElement::new(dom, NodeIndex::new($idx));
                $body
            }
        }
    };
}

fn archive_err(e: ArchiveErr) -> PyErr {
    PyIOError::new_err(e.to_string())
}

fn selector_err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(format!("invalid CSS selector: {e}"))
}

/// A compiled CSS selector list.
///
/// Compile once and reuse across `select()` calls to skip re-parsing the selector
/// for every document — the equivalent of `re.compile` for CSS.
#[pyclass(frozen, module = "htmlarc")]
pub struct Selector {
    inner: OwnedSelectorList,
}

#[pymethods]
impl Selector {
    #[new]
    fn new(css: &str) -> PyResult<Self> {
        let inner = OwnedSelectorList::parse(css).map_err(selector_err)?;
        Ok(Selector { inner })
    }

    /// The selector source text.
    #[getter]
    fn source(&self) -> &str {
        self.inner.source()
    }

    fn __repr__(&self) -> String {
        format!("Selector({:?})", self.inner.source())
    }
}

/// Everywhere a selector is accepted, both a plain CSS string and a pre-compiled
/// [`Selector`] work; strings are compiled on the spot.
#[derive(FromPyObject)]
enum CssArg<'py> {
    Compiled(Bound<'py, Selector>),
    Css(String),
}

impl<'py> CssArg<'py> {
    /// The compiled selector list, parsing into `storage` when given source text (the caller
    /// keeps `storage` alive for the duration of the query).
    fn resolve<'a>(
        &'a self,
        storage: &'a mut Option<OwnedSelectorList>,
    ) -> PyResult<&'a OwnedSelectorList> {
        match self {
            CssArg::Compiled(sel) => Ok(&sel.get().inner),
            CssArg::Css(css) => {
                let parsed = OwnedSelectorList::parse(css.as_str()).map_err(selector_err)?;
                Ok(storage.insert(parsed))
            }
        }
    }
}

/// A parsed HTML document.
///
/// Obtained from `htmlarc.parse(html)` or by indexing an `Archive`; there is no direct
/// constructor. All access goes through elements, starting at `root`.
#[pyclass(frozen, module = "htmlarc")]
pub struct Document {
    backing: Backing,
    key: Option<String>,
}

impl Document {
    fn element(slf: &Bound<'_, Self>, index: NodeIndex) -> Element {
        Element {
            doc: slf.clone().unbind(),
            index: index.as_u32(),
        }
    }
}

#[pymethods]
impl Document {
    /// The archive key this document was stored under, or `None` for parsed documents.
    #[getter]
    fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// The document root element (renders the whole document, selects over all of it).
    #[getter]
    fn root(slf: &Bound<'_, Self>) -> Element {
        Document::element(slf, NodeIndex::ROOT)
    }

    /// All descendant text of the document, concatenated.
    #[getter]
    fn text(&self) -> String {
        with_el!(self, 0, |el| el.text_content())
    }

    /// All elements matching the CSS selector (a string or a compiled `Selector`),
    /// in document order.
    fn select(slf: &Bound<'_, Self>, selector: CssArg<'_>) -> PyResult<Vec<Element>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let this = slf.get();
        let ids: Vec<u32> = with_el!(this, 0, |el| el
            .select(sel.list().clone())
            .map(|e| e.index().as_u32())
            .collect());
        Ok(ids
            .into_iter()
            .map(|i| Document::element(slf, NodeIndex::new(i)))
            .collect())
    }

    /// The first element matching the CSS selector, or `None`.
    fn select_first(slf: &Bound<'_, Self>, selector: CssArg<'_>) -> PyResult<Option<Element>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let this = slf.get();
        let id: Option<u32> = with_el!(this, 0, |el| el
            .select(sel.list().clone())
            .map(|e| e.index().as_u32())
            .next());
        Ok(id.map(|i| Document::element(slf, NodeIndex::new(i))))
    }

    /// Render the document as HTML. `pretty=True` indents; the default is the raw
    /// compact form.
    #[pyo3(signature = (pretty = false))]
    fn to_html(&self, pretty: bool) -> String {
        with_el!(self, 0, |el| el
            .to_html(HtmlFormat::raw_else_pretty(!pretty)))
    }

    fn __repr__(&self) -> String {
        match &self.key {
            Some(key) => format!("<htmlarc.Document key={key:?}>"),
            None => "<htmlarc.Document>".to_string(),
        }
    }
}

/// An element within a [`Document`].
///
/// A lightweight handle (document reference + node index); creating and dropping
/// elements is cheap and never copies the document.
#[pyclass(frozen, module = "htmlarc")]
pub struct Element {
    doc: Py<Document>,
    index: u32,
}

impl Element {
    fn derived(&self, py: Python<'_>, index: NodeIndex) -> Element {
        Element {
            doc: self.doc.clone_ref(py),
            index: index.as_u32(),
        }
    }
}

#[pymethods]
impl Element {
    /// The document this element belongs to.
    #[getter]
    fn document(&self, py: Python<'_>) -> Py<Document> {
        self.doc.clone_ref(py)
    }

    /// The element's node index within the document (stable for the document's lifetime).
    #[getter]
    fn index(&self) -> u32 {
        self.index
    }

    /// The tag name, lowercase (e.g. `"div"`).
    #[getter]
    fn tag(&self) -> String {
        with_el!(self.doc.get(), self.index, |el| el.tag_name().to_string())
    }

    /// The `id` attribute, or `None`.
    #[getter]
    fn id(&self) -> Option<String> {
        with_el!(self.doc.get(), self.index, |el| el
            .id()
            .map(|s| s.to_string()))
    }

    /// The class list, in document order.
    #[getter]
    fn classes(&self) -> Vec<String> {
        with_el!(self.doc.get(), self.index, |el| el
            .classes()
            .map(|c| c.to_string())
            .collect())
    }

    /// All attributes as a dict (entity-decoded values). htmlarc stores the class list
    /// out-of-band, so a `"class"` entry is synthesized from it (space-joined).
    #[getter]
    fn attrs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let pairs: Vec<(String, String)> = with_el!(self.doc.get(), self.index, |el| el
            .attributes()
            .map(|a| (a.name.to_string(), a.val.to_string()))
            .collect());
        let dict = PyDict::new(py);
        let classes = self.classes();
        if !classes.is_empty() {
            dict.set_item("class", classes.join(" "))?;
        }
        for (name, val) in pairs {
            dict.set_item(name, val)?;
        }
        Ok(dict)
    }

    /// The value of the named attribute (ASCII case-insensitive), or `None`.
    /// `"class"` resolves through the class list, like `attrs`.
    fn get(&self, name: &str) -> Option<String> {
        if name.eq_ignore_ascii_case("class") {
            let classes = self.classes();
            return (!classes.is_empty()).then(|| classes.join(" "));
        }
        with_el!(self.doc.get(), self.index, |el| el.get_attribute(name))
    }

    /// `element["href"]` — like `get()`, but raises `KeyError` when absent.
    fn __getitem__(&self, name: &str) -> PyResult<String> {
        self.get(name)
            .ok_or_else(|| PyKeyError::new_err(name.to_string()))
    }

    /// All descendant text, concatenated (like BeautifulSoup's `get_text()`).
    #[getter]
    fn text(&self) -> String {
        with_el!(self.doc.get(), self.index, |el| el.text_content())
    }

    /// The element's own immediate text — its direct text-node children concatenated,
    /// excluding descendants' text — or `None` when it has none.
    #[getter]
    fn own_text(&self) -> Option<String> {
        let text: String = with_el!(self.doc.get(), self.index, |el| el
            .children()
            .set_include_text()
            .filter_map(|c| c.text())
            .collect());
        (!text.is_empty()).then_some(text)
    }

    /// A CSS path locating this element (e.g. `"html > body > div#main > p"`).
    #[getter]
    fn css_path(&self) -> String {
        with_el!(self.doc.get(), self.index, |el| el.css_path())
    }

    /// The parent element, or `None` at the root.
    #[getter]
    fn parent(&self, py: Python<'_>) -> Option<Element> {
        with_el!(self.doc.get(), self.index, |el| el
            .parent()
            .ok()
            .map(|e| e.index()))
        .map(|i| self.derived(py, i))
    }

    /// The next sibling element, or `None`.
    #[getter]
    fn next_sibling(&self, py: Python<'_>) -> Option<Element> {
        with_el!(self.doc.get(), self.index, |el| el
            .next_sibling()
            .ok()
            .map(|e| e.index()))
        .map(|i| self.derived(py, i))
    }

    /// The previous sibling element, or `None`.
    #[getter]
    fn prev_sibling(&self, py: Python<'_>) -> Option<Element> {
        with_el!(self.doc.get(), self.index, |el| el
            .prev_sibling()
            .ok()
            .map(|e| e.index()))
        .map(|i| self.derived(py, i))
    }

    /// The child elements, in document order.
    #[getter]
    fn children(&self, py: Python<'_>) -> Vec<Element> {
        let ids: Vec<NodeIndex> = with_el!(self.doc.get(), self.index, |el| el
            .children()
            .map(|c| c.index())
            .collect());
        ids.into_iter().map(|i| self.derived(py, i)).collect()
    }

    /// All descendant elements matching the CSS selector (a string or a compiled
    /// `Selector`), in document order.
    fn select(&self, py: Python<'_>, selector: CssArg<'_>) -> PyResult<Vec<Element>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let ids: Vec<u32> = with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| e.index().as_u32())
            .collect());
        Ok(ids
            .into_iter()
            .map(|i| self.derived(py, NodeIndex::new(i)))
            .collect())
    }

    /// The first descendant element matching the CSS selector, or `None`.
    fn select_first(&self, py: Python<'_>, selector: CssArg<'_>) -> PyResult<Option<Element>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let id: Option<u32> = with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| e.index().as_u32())
            .next());
        Ok(id.map(|i| self.derived(py, NodeIndex::new(i))))
    }

    /// Whether this element itself matches the CSS selector.
    fn matches(&self, selector: CssArg<'_>) -> PyResult<bool> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self.doc.get(), self.index, |el| el.matches(sel.list())))
    }

    /// Render this element's subtree as HTML. `pretty=True` indents; the default is
    /// the raw compact form.
    #[pyo3(signature = (pretty = false))]
    fn to_html(&self, pretty: bool) -> String {
        with_el!(self.doc.get(), self.index, |el| el
            .to_html(HtmlFormat::raw_else_pretty(!pretty)))
    }

    fn __repr__(&self) -> String {
        let descr = with_el!(self.doc.get(), self.index, |el| el.tag_id_class());
        format!("<htmlarc.Element {descr}>")
    }
}

/// `archive[...]` accepts a position (int) or a key (str).
#[derive(FromPyObject)]
enum DocIndex {
    Pos(isize),
    Key(String),
}

/// A read-only, memory-mapped `.htmlarc` archive.
///
/// Documents are stored pre-parsed: indexing returns a queryable `Document` with no
/// HTML parsing at read time. Index by position (`archive[0]`) or key (`archive["…"]`),
/// or iterate to visit every document.
#[pyclass(frozen, module = "htmlarc")]
pub struct Archive {
    inner: Arc<MmapArchive>,
    path: String,
}

impl Archive {
    fn document(&self, pos: usize) -> PyResult<Document> {
        let owned = OwnedDoc::new(self.inner.clone(), pos).map_err(archive_err)?;
        Ok(Document {
            key: Some(owned.key().to_string()),
            backing: Backing::Archived(owned),
        })
    }
}

#[pymethods]
impl Archive {
    #[new]
    fn new(path: PathBuf) -> PyResult<Self> {
        let inner = MmapArchive::open(&path).map_err(archive_err)?;
        Ok(Archive {
            inner: Arc::new(inner),
            path: path.display().to_string(),
        })
    }

    /// The archive file path.
    #[getter]
    fn path(&self) -> &str {
        &self.path
    }

    /// The number of documents.
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// All document keys, in archive order.
    fn keys(&self) -> Vec<String> {
        self.inner.keys().map(|k| k.to_string()).collect()
    }

    fn __contains__(&self, key: &str) -> bool {
        self.inner.position_for_key(key).is_some()
    }

    /// The document at a position (int, negative indexes from the end) or under a
    /// key (str). Raises `IndexError` / `KeyError` when absent.
    fn __getitem__(&self, index: DocIndex) -> PyResult<Document> {
        match index {
            DocIndex::Pos(i) => {
                let len = self.inner.len() as isize;
                let pos = if i < 0 { i + len } else { i };
                if !(0..len).contains(&pos) {
                    return Err(PyIndexError::new_err(format!(
                        "document index {i} out of range for archive of {len}"
                    )));
                }
                self.document(pos as usize)
            }
            DocIndex::Key(key) => self
                .inner
                .position_for_key(&key)
                .map(|pos| self.document(pos))
                .transpose()?
                .ok_or_else(|| PyKeyError::new_err(key)),
        }
    }

    /// The document under `key`, or `None` when absent.
    fn get(&self, key: &str) -> PyResult<Option<Document>> {
        self.inner
            .position_for_key(key)
            .map(|pos| self.document(pos))
            .transpose()
    }

    /// Iterate over every document, in archive order.
    fn __iter__(&self) -> ArchiveIter {
        ArchiveIter {
            inner: self.inner.clone(),
            pos: 0,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "<htmlarc.Archive path={:?} documents={}>",
            self.path,
            self.inner.len()
        )
    }
}

/// Iterator over an [`Archive`]'s documents.
#[pyclass(module = "htmlarc")]
pub struct ArchiveIter {
    inner: Arc<MmapArchive>,
    pos: usize,
}

#[pymethods]
impl ArchiveIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(&mut self) -> PyResult<Option<Document>> {
        if self.pos >= self.inner.len() {
            return Ok(None);
        }
        let owned = OwnedDoc::new(self.inner.clone(), self.pos).map_err(archive_err)?;
        self.pos += 1;
        Ok(Some(Document {
            key: Some(owned.key().to_string()),
            backing: Backing::Archived(owned),
        }))
    }
}

/// Builds a `.htmlarc` archive from HTML strings.
///
/// Add documents with `add(key, html)` (duplicate keys are skipped, first wins —
/// matching the archive's dedup rule), then `write(path)` once. The builder cannot
/// be reused after writing.
#[pyclass(module = "htmlarc")]
pub struct ArchiveBuilder {
    builder: Option<HtmlArchiveBuilder>,
}

#[pymethods]
impl ArchiveBuilder {
    #[new]
    fn new() -> Self {
        ArchiveBuilder {
            builder: Some(HtmlArchiveBuilder::default()),
        }
    }

    /// Parse `html` and add it under `key`. Raises `ValueError` when the HTML exceeds
    /// htmlarc's per-document capacity or cannot be parsed.
    fn add(&mut self, key: &str, html: &str) -> PyResult<()> {
        let builder = self
            .builder
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        let doc = HtmlDoc::parse(html).map_err(|e| PyValueError::new_err(e.to_string()))?;
        builder.add_html(key.to_string(), doc);
        Ok(())
    }

    /// Write the archive to `path` and consume the builder.
    fn write(&mut self, path: PathBuf) -> PyResult<()> {
        let builder = self
            .builder
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        builder.write_to(path).map_err(archive_err)
    }
}

/// Parse an HTML string into a queryable [`Document`].
///
/// Parsing is fault-tolerant in the html5-recovery sense: malformed markup yields a
/// best-effort tree rather than an error. Raises `ValueError` only when the document
/// exceeds htmlarc's per-document capacity.
#[pyfunction]
fn parse(html: &str) -> PyResult<Document> {
    let doc = HtmlDoc::parse(html).map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(Document {
        backing: Backing::Parsed(Box::new(doc.dom())),
        key: None,
    })
}

/// Open a `.htmlarc` archive (alias for `Archive(path)`).
#[pyfunction]
fn open(path: PathBuf) -> PyResult<Archive> {
    Archive::new(path)
}

/// Query pre-parsed HTML document archives (`.htmlarc`) with CSS selectors.
#[pymodule]
fn htmlarc(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_class::<Archive>()?;
    m.add_class::<ArchiveBuilder>()?;
    m.add_class::<ArchiveIter>()?;
    m.add_class::<Document>()?;
    m.add_class::<Element>()?;
    m.add_class::<Selector>()?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    Ok(())
}
