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

use arrow_array::ffi_stream::FFI_ArrowArrayStream;
use arrow_array::{ArrayRef, RecordBatch, RecordBatchIterator, StringArray};
use arrow_buffer::{Buffer, NullBufferBuilder, OffsetBuffer, ScalarBuffer};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use htmlarc_archive::{ArchiveErr, HtmlArchiveBuilder, MmapArchive, OwnedDoc};
use htmlarc_dom::prelude::{
    DomInner, DomIterator, DomRead, DomRef, HtmlDoc, HtmlElement, HtmlFormat, NodeIndex,
    OwnedSelectorList,
};
use pyo3::exceptions::{
    PyIOError, PyIndexError, PyKeyError, PyRuntimeError, PyTypeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::{PyCapsule, PyDict};

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

/// The named attribute's value with the wrapper's `"class"` semantics: htmlarc stores the
/// class list out-of-band, so `"class"` resolves through it (space-joined) instead of the
/// attr store. Every attribute lookup in the module funnels through here.
fn attr_value<Dom: DomRead + DomRef>(el: &HtmlElement<'_, Dom>, name: &str) -> Option<String> {
    if name.eq_ignore_ascii_case("class") {
        let classes: Vec<_> = el.classes().collect();
        return (!classes.is_empty()).then(|| classes.join(" "));
    }
    el.get_attribute(name)
}

/// Whether `name` is present on `el`, with the wrapper's `"class"` semantics, without
/// materializing the value — the presence counterpart to [`attr_value`].
fn attr_present<Dom: DomRead + DomRef>(el: &HtmlElement<'_, Dom>, name: &str) -> bool {
    if name.eq_ignore_ascii_case("class") {
        el.classes().next().is_some()
    } else {
        el.has_attribute(name)
    }
}

/// The number of elements under `el` matching `sel`; with `attr`, only those carrying the
/// attribute (per [`attr_present`]). Never materializes text and allocates nothing per
/// match, so counting stays on the select-only path — no string-block inflation, no
/// PyObjects, nothing marshalled across the FFI boundary.
fn count_matches<Dom: DomRead + DomRef>(
    el: &HtmlElement<'_, Dom>,
    sel: &OwnedSelectorList,
    attr: Option<&str>,
) -> usize {
    match attr {
        None => el.select(sel.list().clone()).count(),
        Some(name) => el
            .select(sel.list().clone())
            .filter(|e| attr_present(e, name))
            .count(),
    }
}

/// Run `f` over every document in the archive, fanned out across all cores. Returns
/// `(key, value)` for the documents where `f` answered, in archive order. Callers hold no
/// GIL here (`Python::detach`); everything captured must be `Sync`.
fn par_sweep<T: Send>(
    archive: &Arc<MmapArchive>,
    f: impl Fn(&OwnedDoc) -> Option<T> + Sync,
) -> Result<Vec<(String, T)>, ArchiveErr> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let n = archive.len();
    let threads = std::thread::available_parallelism()
        .map_or(1, |p| p.get())
        .min(n.max(1));
    // Work-stealing by atomic counter: document sizes vary wildly, so fixed ranges would
    // leave threads idle behind whoever drew the big documents.
    let next = AtomicUsize::new(0);
    let mut hits: Vec<(usize, String, T)> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|_| {
                s.spawn(|| {
                    let mut local = Vec::new();
                    loop {
                        let pos = next.fetch_add(1, Ordering::Relaxed);
                        if pos >= n {
                            break;
                        }
                        let doc = OwnedDoc::new(archive.clone(), pos)?;
                        if let Some(v) = f(&doc) {
                            local.push((pos, doc.key().to_string(), v));
                        }
                    }
                    Ok(local)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("archive sweep worker panicked"))
            .collect::<Result<Vec<_>, ArchiveErr>>()
            .map(|per_thread| per_thread.into_iter().flatten().collect())
    })?;
    hits.sort_unstable_by_key(|(pos, ..)| *pos);
    Ok(hits.into_iter().map(|(_, key, v)| (key, v)).collect())
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

/// A css kwarg accepts one selector or a list of them.
#[derive(FromPyObject)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

impl OneOrMany {
    fn into_vec(opt: Option<Self>) -> Vec<String> {
        match opt {
            None => Vec::new(),
            Some(OneOrMany::One(s)) => vec![s],
            Some(OneOrMany::Many(v)) => v,
        }
    }
}

/// An include/exclude predicate over archive documents, for `Archive.matching()`.
///
/// A document is kept when it satisfies every include condition (or there are none)
/// and no exclude condition. Multiple selectors in a list AND together; a comma
/// *inside* one selector string is a CSS selector list, i.e. OR. A pure key filter
/// (no css) never touches document bodies at all.
#[pyclass(frozen, module = "htmlarc")]
pub struct Filter {
    inner: htmlarc_archive::Filter,
    repr: String,
}

#[pymethods]
impl Filter {
    #[new]
    #[pyo3(signature = (*, include_css=None, include_keys=None, exclude_css=None, exclude_keys=None))]
    fn new(
        include_css: Option<OneOrMany>,
        include_keys: Option<Vec<String>>,
        exclude_css: Option<OneOrMany>,
        exclude_keys: Option<Vec<String>>,
    ) -> PyResult<Self> {
        let include_css = OneOrMany::into_vec(include_css);
        let exclude_css = OneOrMany::into_vec(exclude_css);
        let include_keys = include_keys.unwrap_or_default();
        let exclude_keys = exclude_keys.unwrap_or_default();
        let mut parts = Vec::new();
        for (name, css, keys) in [
            ("include", &include_css, &include_keys),
            ("exclude", &exclude_css, &exclude_keys),
        ] {
            if !css.is_empty() {
                parts.push(format!("{name}_css={css:?}"));
            }
            if !keys.is_empty() {
                parts.push(format!("{name}_keys=<{} keys>", keys.len()));
            }
        }
        let repr = format!("Filter({})", parts.join(", "));
        let inner = htmlarc_archive::Filter::from_parts(
            include_css,
            include_keys,
            exclude_css,
            exclude_keys,
        )
        .map_err(selector_err)?;
        Ok(Filter { inner, repr })
    }

    fn __repr__(&self) -> &str {
        &self.repr
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

    /// The descendant text of every element matching the selector, one string per match,
    /// in document order. One FFI call instead of a Python loop over `select()`.
    fn select_text(&self, selector: CssArg<'_>) -> PyResult<Vec<String>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self, 0, |el| el
            .select(sel.list().clone())
            .map(|e| e.text_content())
            .collect()))
    }

    /// The named attribute of every element matching the selector (`None` where absent),
    /// in document order. `"class"` resolves like `Element.get()`.
    fn select_attr(&self, selector: CssArg<'_>, name: &str) -> PyResult<Vec<Option<String>>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self, 0, |el| el
            .select(sel.list().clone())
            .map(|e| attr_value(&e, name))
            .collect()))
    }

    /// The number of elements matching the selector; with `attr`, only elements where
    /// that attribute is present (`"class"` resolves like `Element.get()`). Counts
    /// without materializing elements or text.
    #[pyo3(signature = (selector, attr = None))]
    fn select_count(&self, selector: CssArg<'_>, attr: Option<&str>) -> PyResult<usize> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self, 0, |el| count_matches(&el, sel, attr)))
    }

    /// The rendered subtree of every element matching the selector, in document order.
    #[pyo3(signature = (selector, pretty = false))]
    fn select_html(&self, selector: CssArg<'_>, pretty: bool) -> PyResult<Vec<String>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let fmt = HtmlFormat::raw_else_pretty(!pretty);
        Ok(with_el!(self, 0, |el| el
            .select(sel.list().clone())
            .map(|e| e.to_html(fmt))
            .collect()))
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
        with_el!(self.doc.get(), self.index, |el| attr_value(&el, name))
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

    /// The descendant text of every matching descendant element, one string per match,
    /// in document order.
    fn select_text(&self, selector: CssArg<'_>) -> PyResult<Vec<String>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| e.text_content())
            .collect()))
    }

    /// The named attribute of every matching descendant element (`None` where absent),
    /// in document order. `"class"` resolves like `get()`.
    fn select_attr(&self, selector: CssArg<'_>, name: &str) -> PyResult<Vec<Option<String>>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| attr_value(&e, name))
            .collect()))
    }

    /// The number of matching descendant elements; with `attr`, only elements where
    /// that attribute is present (`"class"` resolves like `get()`). Counts without
    /// materializing elements or text.
    #[pyo3(signature = (selector, attr = None))]
    fn select_count(&self, selector: CssArg<'_>, attr: Option<&str>) -> PyResult<usize> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self.doc.get(), self.index, |el| count_matches(
            &el, sel, attr
        )))
    }

    /// The rendered subtree of every matching descendant element, in document order.
    #[pyo3(signature = (selector, pretty = false))]
    fn select_html(&self, selector: CssArg<'_>, pretty: bool) -> PyResult<Vec<String>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        let fmt = HtmlFormat::raw_else_pretty(!pretty);
        Ok(with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| e.to_html(fmt))
            .collect()))
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

/// `Archive.matching()` accepts a [`Filter`] or anything `select()` accepts.
#[derive(FromPyObject)]
enum MatchArg<'py> {
    Filter(Bound<'py, Filter>),
    Selector(CssArg<'py>),
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

    /// The keys of every document matching the predicate, in archive order. Accepts a
    /// CSS selector (string or compiled `Selector`: keep documents with at least one
    /// match, swept across all cores) or a [`Filter`] (include/exclude rules; a pure key
    /// filter never touches document bodies). Runs with the GIL released either way.
    fn matching(&self, py: Python<'_>, selector: MatchArg<'_>) -> PyResult<Vec<String>> {
        let css = match selector {
            MatchArg::Filter(f) => {
                let filter = &f.get().inner;
                return Ok(py.detach(|| {
                    htmlarc_archive::Archive::entries_matching(&*self.inner, filter)
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                }));
            }
            MatchArg::Selector(css) => css,
        };
        let mut storage = None;
        let sel = css.resolve(&mut storage)?;
        py.detach(|| {
            par_sweep(&self.inner, |doc| {
                let root = HtmlElement::new(doc, NodeIndex::ROOT);
                root.select(sel.list().clone())
                    .next()
                    .is_some()
                    .then_some(())
            })
        })
        .map(|hits| hits.into_iter().map(|(key, ())| key).collect())
        .map_err(archive_err)
    }

    /// `(key, texts)` for every document with at least one match: the descendant text of
    /// each matching element, like `Document.select_text` over the whole archive. Runs
    /// across all cores with the GIL released; documents without matches are omitted.
    fn scan_text(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
    ) -> PyResult<Vec<(String, Vec<String>)>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        py.detach(|| {
            par_sweep(&self.inner, |doc| {
                let root = HtmlElement::new(doc, NodeIndex::ROOT);
                let texts: Vec<String> = root
                    .select(sel.list().clone())
                    .map(|e| e.text_content())
                    .collect();
                (!texts.is_empty()).then_some(texts)
            })
        })
        .map_err(archive_err)
    }

    /// `(key, values)` for every document with at least one match: the named attribute of
    /// each matching element (`None` where absent), like `Document.select_attr` over the
    /// whole archive. Runs across all cores with the GIL released; documents without
    /// matches are omitted.
    fn scan_attr(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
        name: &str,
    ) -> PyResult<Vec<(String, Vec<Option<String>>)>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        py.detach(|| {
            par_sweep(&self.inner, |doc| {
                let root = HtmlElement::new(doc, NodeIndex::ROOT);
                let vals: Vec<Option<String>> = root
                    .select(sel.list().clone())
                    .map(|e| attr_value(&e, name))
                    .collect();
                (!vals.is_empty()).then_some(vals)
            })
        })
        .map_err(archive_err)
    }

    /// The total number of matching elements across every document; with `attr`, only
    /// elements where that attribute is present, like `Document.select_count` summed
    /// over the whole archive. Runs across all cores with the GIL released and returns
    /// a single int — nothing is marshalled per match, and counting never touches
    /// document text, so it stays on the select-only fast path.
    #[pyo3(signature = (selector, attr = None))]
    fn scan_count(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
        attr: Option<&str>,
    ) -> PyResult<usize> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        py.detach(|| {
            par_sweep(&self.inner, |doc| {
                let root = HtmlElement::new(doc, NodeIndex::ROOT);
                let n = count_matches(&root, sel, attr);
                (n > 0).then_some(n)
            })
        })
        .map(|hits| hits.iter().map(|(_, n)| n).sum())
        .map_err(archive_err)
    }

    /// Every match across the archive as one flat Arrow table: a `key` column (the document
    /// key, repeated once per matched element), an optional `text` column (each element's text
    /// content, when `text=True`), and one nullable column per name in `attrs` (the attribute
    /// value, `null` where the matched element lacks it; `"class"` is synthesized space-joined
    /// like `Element.get`). One row per matched element, ordered by document then match — the
    /// same ordering as `scan_text`/`scan_attr`. With `text=False` and no `attrs`, it returns a
    /// one-column inventory of which document each match came from.
    ///
    /// The sweep runs across all cores with the GIL released and assembles the Arrow buffers
    /// entirely off-GIL, handing them to Python zero-copy over the Arrow PyCapsule stream
    /// interface (`pyarrow.table(r)`, `polars.DataFrame(r)`, duckdb, ...). No per-match Python
    /// object is created, so this is far faster than `scan_text`/`scan_attr` when extracting
    /// from every document. Raises `ValueError` if a requested attribute name collides with the
    /// `key`/`text` columns or duplicates another (names are matched case-insensitively).
    #[pyo3(signature = (selector, *, text = false, attrs = None))]
    fn scan_table(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
        text: bool,
        attrs: Option<Vec<String>>,
    ) -> PyResult<ArrowResult> {
        let attrs = attrs.unwrap_or_default();

        // Validate column names up front: the schema needs unique, unreserved labels or the
        // table would silently collide columns.
        for name in &attrs {
            if name.eq_ignore_ascii_case("key") {
                return Err(PyValueError::new_err(
                    "scan_table: attribute name \"key\" collides with the key column",
                ));
            }
            if text && name.eq_ignore_ascii_case("text") {
                return Err(PyValueError::new_err(
                    "scan_table: attribute name \"text\" collides with the text column (text=True)",
                ));
            }
        }
        for (i, a) in attrs.iter().enumerate() {
            if attrs[..i].iter().any(|b| a.eq_ignore_ascii_case(b)) {
                return Err(PyValueError::new_err(format!(
                    "scan_table: duplicate attribute column {a:?} (names match case-insensitively)"
                )));
            }
        }

        let cols: Vec<AttrCol> = attrs.iter().map(|n| AttrCol::new(n)).collect();

        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;

        // Schema: key (non-null), optional text (non-null), one nullable utf8 column per attr.
        let mut fields: Vec<Field> = Vec::with_capacity(2 + cols.len());
        fields.push(Field::new("key", DataType::Utf8, false));
        if text {
            fields.push(Field::new("text", DataType::Utf8, false));
        }
        for a in &cols {
            fields.push(Field::new(&a.name, DataType::Utf8, true));
        }
        let schema: SchemaRef = Arc::new(Schema::new(fields));

        let hits = py
            .detach(|| {
                par_sweep(&self.inner, |doc| {
                    let root = HtmlElement::new(doc, NodeIndex::ROOT);
                    scan_table_doc(&root, sel, text, &cols)
                })
            })
            .map_err(archive_err)?;

        let batches = py
            .detach(|| build_batches(hits, text, &cols, &schema))
            .map_err(arrow_err)?;

        Ok(ArrowResult { schema, batches })
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

// ---- Arrow columnar scan (`Archive.scan_table`) ------------------------------------------

/// Maps an assembly `ArrowError` to a Python `ValueError`. The only failure mode is a single
/// document whose text/attribute column alone exceeds the 2 GiB utf8 offset limit (see
/// [`build_batches`]).
fn arrow_err(e: ArrowError) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// One requested attribute column. `name` is the column label exactly as requested; `class`
/// switches to the synthesized class semantics (out-of-band store, space-joined, absent when the
/// class list is empty), matching [`attr_value`].
struct AttrCol {
    name: String,
    class: bool,
}

impl AttrCol {
    fn new(name: &str) -> Self {
        AttrCol {
            name: name.to_string(),
            class: name.eq_ignore_ascii_case("class"),
        }
    }
}

/// One string column's bytes for a single document, built off-GIL: `data` is the matched values
/// concatenated, `ends` the doc-relative end offset of each row (row `i` spans `ends[i-1]..ends[i]`,
/// with an implicit `0` start). `i64` so a worker can never overflow before the batch rebase.
#[derive(Default)]
struct ColChunk {
    data: Vec<u8>,
    ends: Vec<i64>,
}

impl ColChunk {
    fn push_end(&mut self) {
        self.ends.push(self.data.len() as i64);
    }
}

/// A single document's contribution to the table: `rows` matched elements, plus the bytes for the
/// optional text column and each attribute column (with per-row validity for attributes — a
/// missing attribute is a null, distinct from a present-but-empty `""`).
struct DocChunk {
    rows: usize,
    text: Option<ColChunk>,
    attrs: Vec<(ColChunk, Vec<bool>)>,
}

/// Build one document's row contributions off-GIL (runs inside `par_sweep`). Returns `None` when
/// nothing matched, so documents without matches are omitted — exactly like the other sweeps.
fn scan_table_doc(
    root: &HtmlElement<'_, OwnedDoc>,
    sel: &OwnedSelectorList,
    want_text: bool,
    cols: &[AttrCol],
) -> Option<DocChunk> {
    let mut text = want_text.then(ColChunk::default);
    let mut attrs: Vec<(ColChunk, Vec<bool>)> = cols
        .iter()
        .map(|_| (ColChunk::default(), Vec::new()))
        .collect();
    let mut rows = 0usize;

    for el in root.select(sel.list().clone()) {
        rows += 1;
        if let Some(t) = text.as_mut() {
            el.for_each_text_chunk(|s| t.data.extend_from_slice(s.as_bytes()));
            t.push_end();
        }
        for (col, (chunk, valid)) in cols.iter().zip(attrs.iter_mut()) {
            let present = if col.class {
                // Synthesize "class" from the out-of-band store, space-joined; empty list => null,
                // matching attr_value's None.
                let mut any = false;
                for cls in el.classes() {
                    if any {
                        chunk.data.push(b' ');
                    }
                    chunk.data.extend_from_slice(cls.as_bytes());
                    any = true;
                }
                any
            } else {
                el.with_attribute(&col.name, |v| match v {
                    Some(s) => {
                        chunk.data.extend_from_slice(s.as_bytes());
                        true
                    }
                    None => false,
                })
            };
            chunk.push_end();
            valid.push(present);
        }
    }

    (rows > 0).then_some(DocChunk { rows, text, attrs })
}

/// Cut a fresh RecordBatch at a document boundary whenever a column's utf8 data buffer would pass
/// this many bytes, keeping each column safely under the i32 (2 GiB) offset ceiling. Overridable
/// via `HTMLARC_SCAN_TABLE_BATCH_BYTES` — mainly so tests can force the multi-batch path on a
/// small corpus, but also a tuning knob for callers who want smaller batches.
fn batch_data_limit() -> usize {
    const DEFAULT: usize = 1 << 30;
    match std::env::var("HTMLARC_SCAN_TABLE_BATCH_BYTES") {
        Ok(v) => v.trim().parse().unwrap_or(DEFAULT),
        Err(_) => DEFAULT,
    }
}

/// One string column under construction within the current batch. `ends` carries the leading `0`
/// offset; one further entry is pushed per appended row.
struct ColBuilder {
    data: Vec<u8>,
    ends: Vec<i32>,
    nulls: Option<NullBufferBuilder>,
}

impl ColBuilder {
    fn new(nullable: bool) -> Self {
        ColBuilder {
            data: Vec::new(),
            ends: vec![0],
            nulls: nullable.then(|| NullBufferBuilder::new(0)),
        }
    }

    /// Finish the current batch's column into a zero-copy `StringArray` and reset for the next
    /// batch (`NullBufferBuilder::finish` already resets itself; `mem::take`/`replace` clear the
    /// data and offsets).
    fn finish(&mut self) -> Result<StringArray, ArrowError> {
        let offsets = OffsetBuffer::new(ScalarBuffer::from(std::mem::replace(
            &mut self.ends,
            vec![0],
        )));
        let values = Buffer::from(std::mem::take(&mut self.data));
        let nulls = self.nulls.as_mut().and_then(|n| n.finish());
        // Every value byte was copied out of a `&str` and every offset is a row boundary
        // recorded in append order, so utf8 validity and offset monotonicity hold by
        // construction; `try_new` would re-validate the entire data buffer (O(bytes), ~6 ms
        // on an 84 MB result). Debug builds still run the full check.
        #[cfg(debug_assertions)]
        StringArray::try_new(offsets.clone(), values.clone(), nulls.clone())?;
        Ok(unsafe { StringArray::new_unchecked(offsets, values, nulls) })
    }
}

/// Append a document's column chunk into the current batch builder, rebasing the doc-relative end
/// offsets onto the batch's running i32 offset. Errors only if a single document's column alone
/// exceeds the 2 GiB utf8 offset ceiling. Ends are non-decreasing, so checking the last against
/// i32 covers them all and the rebase loop needs no per-row branch.
fn append_col(b: &mut ColBuilder, c: &ColChunk, doc_key: &str) -> Result<(), ArrowError> {
    let base = b.data.len() as i64;
    b.data.extend_from_slice(&c.data);
    let last = base + c.ends.last().copied().unwrap_or(0);
    if i32::try_from(last).is_err() {
        return Err(oversize(doc_key));
    }
    b.ends.extend(c.ends.iter().map(|&end| (base + end) as i32));
    Ok(())
}

fn oversize(doc_key: &str) -> ArrowError {
    ArrowError::ComputeError(format!(
        "scan_table: a single column for document {doc_key:?} exceeds the 2 GiB utf8 offset limit"
    ))
}

/// Finish every column builder into a `RecordBatch` (and reset the builders for the next batch).
fn finish_batch(
    schema: &SchemaRef,
    key: &mut ColBuilder,
    text: &mut Option<ColBuilder>,
    attr_builders: &mut [ColBuilder],
) -> Result<RecordBatch, ArrowError> {
    let mut columns: Vec<ArrayRef> = Vec::with_capacity(2 + attr_builders.len());
    columns.push(Arc::new(key.finish()?) as ArrayRef);
    if let Some(t) = text.as_mut() {
        columns.push(Arc::new(t.finish()?) as ArrayRef);
    }
    for b in attr_builders.iter_mut() {
        columns.push(Arc::new(b.finish()?) as ArrayRef);
    }
    RecordBatch::try_new(schema.clone(), columns)
}

/// Concatenate the per-document chunks (already in archive order) into one or more `RecordBatch`es,
/// cutting at document boundaries so no column's utf8 data passes [`batch_data_limit`]. Serial,
/// still off-GIL. Zero matches yields an empty `Vec` — the stream still carries the schema, so
/// consumers get a correct empty table.
fn build_batches(
    hits: Vec<(String, DocChunk)>,
    want_text: bool,
    cols: &[AttrCol],
    schema: &SchemaRef,
) -> Result<Vec<RecordBatch>, ArrowError> {
    let mut key = ColBuilder::new(false);
    let mut text = want_text.then(|| ColBuilder::new(false));
    let mut attr_builders: Vec<ColBuilder> = cols.iter().map(|_| ColBuilder::new(true)).collect();

    // Reserve each column's exact final size up front (totals are known from the sweep), so
    // the appends below never pay Vec growth reallocations. Over-reservation when the batch
    // cuts early is harmless — cuts only happen near the GiB-scale limit.
    let total_rows: usize = hits.iter().map(|(_, c)| c.rows).sum();
    key.data
        .reserve(hits.iter().map(|(k, c)| k.len() * c.rows).sum());
    key.ends.reserve(total_rows);
    if let Some(t) = text.as_mut() {
        t.data.reserve(
            hits.iter()
                .map(|(_, c)| c.text.as_ref().map_or(0, |t| t.data.len()))
                .sum(),
        );
        t.ends.reserve(total_rows);
    }
    for (i, b) in attr_builders.iter_mut().enumerate() {
        b.data
            .reserve(hits.iter().map(|(_, c)| c.attrs[i].0.data.len()).sum());
        b.ends.reserve(total_rows);
    }

    let mut batches = Vec::new();
    let mut rows_in_batch = 0usize;
    let limit = batch_data_limit();

    for (doc_key, chunk) in hits {
        // Cut before appending if any column would pass the limit and there is something to flush.
        if rows_in_batch > 0 {
            let key_would = key.data.len() + doc_key.len() * chunk.rows;
            let text_would = match (text.as_ref(), chunk.text.as_ref()) {
                (Some(b), Some(c)) => b.data.len() + c.data.len(),
                _ => 0,
            };
            let attr_would = attr_builders
                .iter()
                .zip(chunk.attrs.iter())
                .map(|(b, (c, _))| b.data.len() + c.data.len())
                .max()
                .unwrap_or(0);
            if key_would > limit || text_would > limit || attr_would > limit {
                batches.push(finish_batch(
                    schema,
                    &mut key,
                    &mut text,
                    &mut attr_builders,
                )?);
                rows_in_batch = 0;
            }
        }

        // Key column: repeat the document key once per matched row. Extend by doubling from
        // the repeats already written so the copy is O(log rows) large memcpys instead of
        // `rows` tiny ones, and compute the evenly spaced offsets arithmetically (one
        // overflow check for the whole document instead of one per row).
        let base = key.data.len();
        let klen = doc_key.len();
        let target = base + klen * chunk.rows;
        if i32::try_from(target).is_err() {
            return Err(oversize(&doc_key));
        }
        key.data.reserve(target - base);
        key.data
            .extend_from_slice(&doc_key.as_bytes()[..klen.min(target - base)]);
        while key.data.len() < target {
            let n = (target - key.data.len()).min(key.data.len() - base);
            key.data.extend_from_within(base..base + n);
        }
        key.ends
            .extend((1..=chunk.rows).map(|i| (base + i * klen) as i32));

        // Text column.
        if let (Some(b), Some(c)) = (text.as_mut(), chunk.text.as_ref()) {
            append_col(b, c, &doc_key)?;
        }

        // Attribute columns (data + per-row validity).
        for (b, (c, valid)) in attr_builders.iter_mut().zip(chunk.attrs.iter()) {
            append_col(b, c, &doc_key)?;
            let nulls = b.nulls.as_mut().expect("attr columns are nullable");
            nulls.append_slice(valid);
        }

        rows_in_batch += chunk.rows;
    }

    if rows_in_batch > 0 {
        batches.push(finish_batch(
            schema,
            &mut key,
            &mut text,
            &mut attr_builders,
        )?);
    }

    Ok(batches)
}

/// A columnar scan result ([`Archive::scan_table`]), exported zero-copy over the Arrow PyCapsule
/// *stream* interface. Consume it with `pyarrow.table(r)`, `polars.DataFrame(r)`,
/// `pandas.DataFrame.from_arrow(r)`, `duckdb.sql("... from r")`, or any other
/// `__arrow_c_stream__` reader — htmlarc itself carries no Python-side Arrow dependency. The
/// result is re-consumable: each call exports a fresh stream over the same buffers.
#[pyclass(frozen, module = "htmlarc")]
pub struct ArrowResult {
    schema: SchemaRef,
    batches: Vec<RecordBatch>,
}

#[pymethods]
impl ArrowResult {
    /// Export the table as an Arrow C stream (a PyCapsule named `"arrow_array_stream"`). The
    /// `requested_schema` hint is accepted and ignored — the schema is fixed by the scan, which
    /// the PyCapsule spec permits.
    #[pyo3(signature = (requested_schema = None))]
    fn __arrow_c_stream__<'py>(
        &self,
        py: Python<'py>,
        requested_schema: Option<Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, PyCapsule>> {
        let _ = requested_schema;
        let reader = RecordBatchIterator::new(
            self.batches.clone().into_iter().map(Ok),
            self.schema.clone(),
        );
        let stream = FFI_ArrowArrayStream::new(Box::new(reader));
        PyCapsule::new_with_value(py, stream, c"arrow_array_stream")
    }

    /// The total number of rows (matched elements) across all batches.
    fn __len__(&self) -> usize {
        self.batches.iter().map(|b| b.num_rows()).sum()
    }

    fn __repr__(&self) -> String {
        let cols: Vec<&str> = self
            .schema
            .fields()
            .iter()
            .map(|f| f.name().as_str())
            .collect();
        format!(
            "<htmlarc.ArrowResult rows={} columns={:?} batches={}>",
            self.batches.iter().map(|b| b.num_rows()).sum::<usize>(),
            cols,
            self.batches.len(),
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

/// Builds a `.htmlarc` archive from HTML strings or parsed documents.
///
/// Add documents with `add(key, html)` or `add_document(key, doc)` (duplicate keys
/// are skipped, first wins — matching the archive's dedup rule), then `write(path)`
/// once. The builder cannot be reused after writing.
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

    /// Add an already-parsed `Document` under `key` without re-parsing — the path for
    /// crawlers that parse each page anyway (e.g. for link discovery): parse once, query
    /// for links, then store the same `Document`.
    ///
    /// Accepts documents from `parse()`. Documents handed out by an `Archive` are backed
    /// by shared per-bundle storage and can't be re-added directly; raises `TypeError`
    /// for those (round-trip through `add(key, doc.to_html())` instead).
    fn add_document(&mut self, key: &str, doc: Bound<'_, Document>) -> PyResult<()> {
        let builder = self
            .builder
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        match &doc.get().backing {
            Backing::Parsed(dom) => {
                builder.add_html(key.to_string(), HtmlDoc::from(dom.as_ref().clone()));
                Ok(())
            }
            Backing::Archived(_) => Err(PyTypeError::new_err(
                "document is archive-backed (its text lives in shared bundle storage); \
                 use add(key, doc.to_html()) to copy it into a new archive",
            )),
        }
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
    m.add_class::<ArrowResult>()?;
    m.add_class::<Document>()?;
    m.add_class::<Element>()?;
    m.add_class::<Filter>()?;
    m.add_class::<Selector>()?;
    m.add_function(wrap_pyfunction!(parse, m)?)?;
    m.add_function(wrap_pyfunction!(open, m)?)?;
    Ok(())
}
