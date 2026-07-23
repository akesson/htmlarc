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
use arrow_array::{
    ArrayRef, BooleanArray, Float64Array, Int64Array, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_buffer::{BooleanBuffer, Buffer, NullBufferBuilder, OffsetBuffer, ScalarBuffer};
use arrow_schema::{ArrowError, DataType, Field, Schema, SchemaRef};
use htmlarc_archive::{
    ArchiveErr, HtmlArchiveBuilder, MetaRef, MetaSchema, MetaType, MetaValue, MmapArchive,
    OwnedDoc, archived_value,
};
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

// ---- Typed metadata (ADR 0009): Python <-> core conversions ------------------------------

/// Parse a `{"name": str | int | float | bool}` dict into the core schema. Field names must be
/// unique and must not shadow the reserved `key` column (`meta_table` always emits one).
fn meta_schema_from_py(dict: &Bound<'_, PyDict>) -> PyResult<MetaSchema> {
    use pyo3::types::{PyBool, PyFloat, PyInt, PyString, PyType};

    let py = dict.py();
    let mut fields = Vec::with_capacity(dict.len());
    for (k, v) in dict.iter() {
        let name: String = k
            .extract()
            .map_err(|_| PyTypeError::new_err("meta_schema keys must be field-name strings"))?;
        if name.eq_ignore_ascii_case("key") {
            return Err(PyValueError::new_err(
                "meta_schema field \"key\" collides with the key column",
            ));
        }
        if fields.iter().any(|(n, _): &(String, _)| *n == name) {
            return Err(PyValueError::new_err(format!(
                "duplicate meta_schema field {name:?}"
            )));
        }
        let t = v.cast::<PyType>().map_err(|_| {
            PyTypeError::new_err(format!(
                "meta_schema[{name:?}] must be one of the types str, int, float, bool"
            ))
        })?;
        // `bool` before `int`: bool is a subclass of int in Python.
        let ty = if t.is(py.get_type::<PyBool>()) {
            MetaType::Bool
        } else if t.is(py.get_type::<PyInt>()) {
            MetaType::Int
        } else if t.is(py.get_type::<PyFloat>()) {
            MetaType::Float
        } else if t.is(py.get_type::<PyString>()) {
            MetaType::Str
        } else {
            return Err(PyTypeError::new_err(format!(
                "meta_schema[{name:?}] must be one of the types str, int, float, bool"
            )));
        };
        fields.push((name, ty));
    }
    if fields.is_empty() {
        return Err(PyValueError::new_err(
            "meta_schema must declare at least one field",
        ));
    }
    Ok(MetaSchema { fields })
}

/// Convert a per-document `meta={...}` dict into a schema-ordered row. Missing fields are null;
/// unknown fields are an error (they would silently vanish otherwise). `int` values coerce into
/// `float` fields; Python `bool` never coerces into `int` (it is almost always a bug).
fn meta_row_from_py(
    schema: &MetaSchema,
    dict: &Bound<'_, PyDict>,
) -> PyResult<Vec<Option<MetaValue>>> {
    use pyo3::types::PyBool;

    for (k, _) in dict.iter() {
        let name: String = k
            .extract()
            .map_err(|_| PyTypeError::new_err("meta keys must be field-name strings"))?;
        if schema.index_of(&name).is_none() {
            return Err(PyValueError::new_err(format!(
                "meta field {name:?} is not in the archive's meta_schema"
            )));
        }
    }
    schema
        .fields
        .iter()
        .map(|(name, ty)| {
            let Some(v) = dict.get_item(name)? else {
                return Ok(None);
            };
            if v.is_none() {
                return Ok(None);
            }
            let is_bool = v.is_instance_of::<PyBool>();
            let wrong = |got: &str| {
                PyTypeError::new_err(format!(
                    "meta field {name:?} is declared {}, got {got}",
                    ty.name()
                ))
            };
            let value = match ty {
                MetaType::Bool => {
                    if !is_bool {
                        return Err(wrong(v.get_type().name()?.to_str()?));
                    }
                    MetaValue::Bool(v.extract::<bool>()?)
                }
                MetaType::Int => {
                    if is_bool {
                        return Err(wrong("bool"));
                    }
                    MetaValue::Int(v.extract::<i64>().map_err(|_| {
                        wrong(
                            v.get_type()
                                .name()
                                .map_or("?".into(), |n| n.to_string())
                                .as_str(),
                        )
                    })?)
                }
                MetaType::Float => {
                    if is_bool {
                        return Err(wrong("bool"));
                    }
                    MetaValue::Float(v.extract::<f64>().map_err(|_| {
                        wrong(
                            v.get_type()
                                .name()
                                .map_or("?".into(), |n| n.to_string())
                                .as_str(),
                        )
                    })?)
                }
                MetaType::Str => MetaValue::Str(v.extract::<String>().map_err(|_| {
                    wrong(
                        v.get_type()
                            .name()
                            .map_or("?".into(), |n| n.to_string())
                            .as_str(),
                    )
                })?),
            };
            Ok(Some(value))
        })
        .collect()
}

fn meta_ref_to_py(py: Python<'_>, v: Option<MetaRef<'_>>) -> PyResult<Py<PyAny>> {
    use pyo3::IntoPyObjectExt;
    match v {
        None => Ok(py.None()),
        Some(MetaRef::Str(s)) => s.into_py_any(py),
        Some(MetaRef::Int(x)) => x.into_py_any(py),
        Some(MetaRef::Float(x)) => x.into_py_any(py),
        Some(MetaRef::Bool(x)) => x.into_py_any(py),
    }
}

/// The Python type object corresponding to a metadata field type.
fn meta_type_to_py(py: Python<'_>, ty: MetaType) -> Bound<'_, pyo3::types::PyType> {
    use pyo3::types::{PyBool, PyFloat, PyInt, PyString};
    match ty {
        MetaType::Str => py.get_type::<PyString>(),
        MetaType::Int => py.get_type::<PyInt>(),
        MetaType::Float => py.get_type::<PyFloat>(),
        MetaType::Bool => py.get_type::<PyBool>(),
    }
}

fn meta_arrow_type(ty: MetaType) -> DataType {
    match ty {
        MetaType::Str => DataType::Utf8,
        MetaType::Int => DataType::Int64,
        MetaType::Float => DataType::Float64,
        MetaType::Bool => DataType::Boolean,
    }
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

    /// The document's typed metadata row as a dict (declared fields only, `None` where
    /// null), or `None` for parsed documents and archives without metadata.
    #[getter]
    fn meta(&self, py: Python<'_>) -> PyResult<Option<Py<PyDict>>> {
        let Backing::Archived(doc) = &self.backing else {
            return Ok(None);
        };
        let Some(table) = doc.archive().meta() else {
            return Ok(None);
        };
        let pos = doc.position();
        let dict = PyDict::new(py);
        for (i, name) in table.names.iter().enumerate() {
            let value = archived_value(&table.columns[i], pos);
            dict.set_item(name.as_str(), meta_ref_to_py(py, value)?)?;
        }
        Ok(Some(dict.into()))
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
    /// in document order. `"class"` resolves like `Element.get()`. With
    /// `skip_missing=True`, elements lacking the attribute are dropped instead and the
    /// result is `list[str]` — for selectors like `a[href]` that guarantee its presence.
    #[pyo3(signature = (selector, name, *, skip_missing = false))]
    fn select_attr(
        &self,
        selector: CssArg<'_>,
        name: &str,
        skip_missing: bool,
    ) -> PyResult<Vec<Option<String>>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self, 0, |el| el
            .select(sel.list().clone())
            .map(|e| attr_value(&e, name))
            .filter(|v| !skip_missing || v.is_some())
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
    /// Text inside nested `<script>`/`<style>` is excluded — unless this element
    /// itself is the script/style, whose payload is returned.
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
    /// in document order. `"class"` resolves like `get()`. With `skip_missing=True`,
    /// elements lacking the attribute are dropped instead and the result is `list[str]`.
    #[pyo3(signature = (selector, name, *, skip_missing = false))]
    fn select_attr(
        &self,
        selector: CssArg<'_>,
        name: &str,
        skip_missing: bool,
    ) -> PyResult<Vec<Option<String>>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        Ok(with_el!(self.doc.get(), self.index, |el| el
            .select(sel.list().clone())
            .map(|e| attr_value(&e, name))
            .filter(|v| !skip_missing || v.is_some())
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
/// or iterate to visit every document. The `scan_*`/`matching` sweeps run across all
/// cores with the GIL released — prefer them over a Python loop when extracting from
/// every document.
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

    /// The archive's metadata schema as a `{"name": type}` dict (types
    /// `str`/`int`/`float`/`bool`, declaration order), or `None` when the archive
    /// carries no metadata.
    #[getter]
    fn meta_schema<'py>(&self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyDict>>> {
        let Some(schema) = self.inner.meta_schema() else {
            return Ok(None);
        };
        let dict = PyDict::new(py);
        for (name, ty) in &schema.fields {
            dict.set_item(name, meta_type_to_py(py, *ty))?;
        }
        Ok(Some(dict))
    }

    /// The whole metadata table as one Arrow table: a `key` column plus one **typed**
    /// column per schema field (`str`→utf8, `int`→int64, `float`→float64,
    /// `bool`→boolean, nulls preserved), one row per document in archive order. The
    /// in-archive replacement for a sidecar dataframe: `polars.DataFrame(arc.meta_table())`.
    /// Raises `ValueError` when the archive carries no metadata.
    fn meta_table(&self, py: Python<'_>) -> PyResult<ArrowResult> {
        let inner = self.inner.clone();
        let Some(schema_fields) = inner.meta_schema() else {
            return Err(PyValueError::new_err("archive carries no metadata"));
        };

        let mut fields: Vec<Field> = Vec::with_capacity(1 + schema_fields.fields.len());
        fields.push(Field::new("key", DataType::Utf8, false));
        for (name, ty) in &schema_fields.fields {
            fields.push(Field::new(name, meta_arrow_type(*ty), true));
        }
        let schema: SchemaRef = Arc::new(Schema::new(fields));

        let batch = py
            .detach(|| {
                let table = inner.meta().expect("checked above");
                let n = inner.len();

                let mut key = ColBuilder::new(false);
                for k in inner.keys() {
                    key.data.extend_from_slice(k.as_bytes());
                    if i32::try_from(key.data.len()).is_err() {
                        return Err(oversize(k));
                    }
                    key.ends.push(key.data.len() as i32);
                }

                let mut columns: Vec<ArrayRef> = Vec::with_capacity(1 + table.columns.len());
                columns.push(Arc::new(key.finish()?) as ArrayRef);
                for (ci, (_, ty)) in schema_fields.fields.iter().enumerate() {
                    let mut b = MetaColBuilder::new(*ty);
                    for row in 0..n {
                        b.append(archived_value(&table.columns[ci], row), 1)?;
                    }
                    columns.push(b.finish()?);
                }
                RecordBatch::try_new(schema.clone(), columns)
            })
            .map_err(arrow_err)?;

        Ok(ArrowResult {
            schema,
            batches: vec![batch],
        })
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
    /// matches are omitted. With `skip_missing=True`, elements lacking the attribute are
    /// dropped instead and the value lists are `list[str]`.
    #[pyo3(signature = (selector, name, *, skip_missing = false))]
    fn scan_attr(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
        name: &str,
        skip_missing: bool,
    ) -> PyResult<Vec<(String, Vec<Option<String>>)>> {
        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;
        py.detach(|| {
            par_sweep(&self.inner, |doc| {
                let root = HtmlElement::new(doc, NodeIndex::ROOT);
                let vals: Vec<Option<String>> = root
                    .select(sel.list().clone())
                    .map(|e| attr_value(&e, name))
                    .filter(|v| !skip_missing || v.is_some())
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
    ///
    /// `meta=[...]` appends the named metadata fields as **typed** columns (`str`→utf8,
    /// `int`→int64, `float`→float64, `bool`→boolean): each row carries its document's value,
    /// so the result needs no join back to `meta_table()`. Raises `ValueError` when the
    /// archive carries no metadata, a name is not in the schema, or it collides with
    /// another column.
    #[pyo3(signature = (selector, *, text = false, attrs = None, meta = None))]
    fn scan_table(
        &self,
        py: Python<'_>,
        selector: CssArg<'_>,
        text: bool,
        attrs: Option<Vec<String>>,
        meta: Option<Vec<String>>,
    ) -> PyResult<ArrowResult> {
        let attrs = attrs.unwrap_or_default();
        let meta = meta.unwrap_or_default();

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

        // Metadata columns: each name must exist in the archive's schema and not collide
        // with the key/text/attr columns (or another meta column).
        let meta_fields: Vec<(String, usize, MetaType)> = if meta.is_empty() {
            Vec::new()
        } else {
            let Some(schema) = self.inner.meta_schema() else {
                return Err(PyValueError::new_err(
                    "scan_table: meta requested but the archive carries no metadata",
                ));
            };
            let mut out: Vec<(String, usize, MetaType)> = Vec::with_capacity(meta.len());
            for name in &meta {
                let Some(idx) = schema.index_of(name) else {
                    return Err(PyValueError::new_err(format!(
                        "scan_table: meta field {name:?} is not in the archive's meta_schema"
                    )));
                };
                if name.eq_ignore_ascii_case("key")
                    || (text && name.eq_ignore_ascii_case("text"))
                    || attrs.iter().any(|a| a.eq_ignore_ascii_case(name))
                    || out.iter().any(|(n, ..)| n.eq_ignore_ascii_case(name))
                {
                    return Err(PyValueError::new_err(format!(
                        "scan_table: meta column {name:?} collides with another column"
                    )));
                }
                out.push((name.clone(), idx, schema.fields[idx].1));
            }
            out
        };

        let cols: Vec<AttrCol> = attrs.iter().map(|n| AttrCol::new(n)).collect();

        let mut storage = None;
        let sel = selector.resolve(&mut storage)?;

        // Schema: key (non-null), optional text (non-null), one nullable utf8 column per
        // attr, one typed nullable column per meta field.
        let mut fields: Vec<Field> = Vec::with_capacity(2 + cols.len() + meta_fields.len());
        fields.push(Field::new("key", DataType::Utf8, false));
        if text {
            fields.push(Field::new("text", DataType::Utf8, false));
        }
        for a in &cols {
            fields.push(Field::new(&a.name, DataType::Utf8, true));
        }
        for (name, _, ty) in &meta_fields {
            fields.push(Field::new(name, meta_arrow_type(*ty), true));
        }
        let schema: SchemaRef = Arc::new(Schema::new(fields));

        let hits = py
            .detach(|| {
                par_sweep(&self.inner, |doc| {
                    let root = HtmlElement::new(doc, NodeIndex::ROOT);
                    scan_table_doc(&root, sel, text, &cols).map(|mut c| {
                        c.pos = doc.position();
                        c
                    })
                })
            })
            .map_err(archive_err)?;

        let archive = self.inner.clone();
        let batches = py
            .detach(|| build_batches(hits, text, &cols, &meta_fields, &archive, &schema))
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
    /// The document's flat archive position — resolves its metadata row for `meta=[...]`
    /// columns (stamped by the sweep closure, which owns the `OwnedDoc`).
    pos: usize,
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

    (rows > 0).then_some(DocChunk {
        rows,
        pos: 0,
        text,
        attrs,
    })
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

/// One **typed** metadata column under construction (`scan_table(meta=[...])` and
/// `meta_table`). The str variant reuses [`ColBuilder`]; the scalar variants build their
/// value/validity buffers directly — nothing here is per-match work, a document's value is
/// fetched once and appended `rows` times.
enum MetaColBuilder {
    Str(ColBuilder),
    Int {
        values: Vec<i64>,
        nulls: NullBufferBuilder,
    },
    Float {
        values: Vec<f64>,
        nulls: NullBufferBuilder,
    },
    Bool {
        values: Vec<bool>,
        nulls: NullBufferBuilder,
    },
}

impl MetaColBuilder {
    fn new(ty: MetaType) -> Self {
        match ty {
            MetaType::Str => MetaColBuilder::Str(ColBuilder::new(true)),
            MetaType::Int => MetaColBuilder::Int {
                values: Vec::new(),
                nulls: NullBufferBuilder::new(0),
            },
            MetaType::Float => MetaColBuilder::Float {
                values: Vec::new(),
                nulls: NullBufferBuilder::new(0),
            },
            MetaType::Bool => MetaColBuilder::Bool {
                values: Vec::new(),
                nulls: NullBufferBuilder::new(0),
            },
        }
    }

    /// Append `repeat` rows of `value`. A mismatched variant cannot occur (values come from a
    /// column of the builder's own declared type) and is treated as null.
    fn append(&mut self, value: Option<MetaRef<'_>>, repeat: usize) -> Result<(), ArrowError> {
        match self {
            MetaColBuilder::Str(b) => {
                let nulls = b.nulls.as_mut().expect("meta str columns are nullable");
                if let Some(MetaRef::Str(s)) = value {
                    for _ in 0..repeat {
                        b.data.extend_from_slice(s.as_bytes());
                        if i32::try_from(b.data.len()).is_err() {
                            return Err(ArrowError::ComputeError(
                                "scan_table: a metadata column exceeds the 2 GiB utf8 offset limit"
                                    .to_string(),
                            ));
                        }
                        b.ends.push(b.data.len() as i32);
                    }
                    nulls.append_n_non_nulls(repeat);
                } else {
                    let end = *b.ends.last().expect("ends starts with 0");
                    b.ends.extend(std::iter::repeat_n(end, repeat));
                    nulls.append_n_nulls(repeat);
                }
            }
            MetaColBuilder::Int { values, nulls } => {
                if let Some(MetaRef::Int(x)) = value {
                    values.extend(std::iter::repeat_n(x, repeat));
                    nulls.append_n_non_nulls(repeat);
                } else {
                    values.extend(std::iter::repeat_n(0, repeat));
                    nulls.append_n_nulls(repeat);
                }
            }
            MetaColBuilder::Float { values, nulls } => {
                if let Some(MetaRef::Float(x)) = value {
                    values.extend(std::iter::repeat_n(x, repeat));
                    nulls.append_n_non_nulls(repeat);
                } else {
                    values.extend(std::iter::repeat_n(0.0, repeat));
                    nulls.append_n_nulls(repeat);
                }
            }
            MetaColBuilder::Bool { values, nulls } => {
                if let Some(MetaRef::Bool(x)) = value {
                    values.extend(std::iter::repeat_n(x, repeat));
                    nulls.append_n_non_nulls(repeat);
                } else {
                    values.extend(std::iter::repeat_n(false, repeat));
                    nulls.append_n_nulls(repeat);
                }
            }
        }
        Ok(())
    }

    /// Current utf8 data size (str variant only) — feeds the batch-cut check.
    fn data_len(&self) -> usize {
        match self {
            MetaColBuilder::Str(b) => b.data.len(),
            _ => 0,
        }
    }

    /// Finish the current batch's column and reset for the next batch.
    fn finish(&mut self) -> Result<ArrayRef, ArrowError> {
        Ok(match self {
            MetaColBuilder::Str(b) => Arc::new(b.finish()?) as ArrayRef,
            MetaColBuilder::Int { values, nulls } => Arc::new(Int64Array::new(
                ScalarBuffer::from(std::mem::take(values)),
                nulls.finish(),
            )) as ArrayRef,
            MetaColBuilder::Float { values, nulls } => Arc::new(Float64Array::new(
                ScalarBuffer::from(std::mem::take(values)),
                nulls.finish(),
            )) as ArrayRef,
            MetaColBuilder::Bool { values, nulls } => Arc::new(BooleanArray::new(
                std::mem::take(values)
                    .into_iter()
                    .collect::<BooleanBuffer>(),
                nulls.finish(),
            )) as ArrayRef,
        })
    }
}

/// Finish every column builder into a `RecordBatch` (and reset the builders for the next batch).
fn finish_batch(
    schema: &SchemaRef,
    key: &mut ColBuilder,
    text: &mut Option<ColBuilder>,
    attr_builders: &mut [ColBuilder],
    meta_builders: &mut [MetaColBuilder],
) -> Result<RecordBatch, ArrowError> {
    let mut columns: Vec<ArrayRef> =
        Vec::with_capacity(2 + attr_builders.len() + meta_builders.len());
    columns.push(Arc::new(key.finish()?) as ArrayRef);
    if let Some(t) = text.as_mut() {
        columns.push(Arc::new(t.finish()?) as ArrayRef);
    }
    for b in attr_builders.iter_mut() {
        columns.push(Arc::new(b.finish()?) as ArrayRef);
    }
    for b in meta_builders.iter_mut() {
        columns.push(b.finish()?);
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
    meta_fields: &[(String, usize, MetaType)],
    archive: &MmapArchive,
    schema: &SchemaRef,
) -> Result<Vec<RecordBatch>, ArrowError> {
    let mut key = ColBuilder::new(false);
    let mut text = want_text.then(|| ColBuilder::new(false));
    let mut attr_builders: Vec<ColBuilder> = cols.iter().map(|_| ColBuilder::new(true)).collect();
    let mut meta_builders: Vec<MetaColBuilder> = meta_fields
        .iter()
        .map(|(_, _, ty)| MetaColBuilder::new(*ty))
        .collect();
    let meta_table = archive.meta();

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
            let meta_now = meta_builders
                .iter()
                .map(|b| b.data_len())
                .max()
                .unwrap_or(0);
            if key_would > limit || text_would > limit || attr_would > limit || meta_now > limit {
                batches.push(finish_batch(
                    schema,
                    &mut key,
                    &mut text,
                    &mut attr_builders,
                    &mut meta_builders,
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

        // Metadata columns: fetch the document's value once, append it `rows` times.
        if !meta_fields.is_empty() {
            let table = meta_table.expect("meta_fields validated against the archive schema");
            for (b, (_, field_idx, _)) in meta_builders.iter_mut().zip(meta_fields.iter()) {
                let value = htmlarc_archive::archived_value(&table.columns[*field_idx], chunk.pos);
                b.append(value, chunk.rows)?;
            }
        }

        rows_in_batch += chunk.rows;
    }

    if rows_in_batch > 0 {
        batches.push(finish_batch(
            schema,
            &mut key,
            &mut text,
            &mut attr_builders,
            &mut meta_builders,
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
///
/// With a `path` at construction the builder is a context manager: `with
/// htmlarc.ArchiveBuilder("out.htmlarc") as b:` writes on clean exit and skips
/// the write when the block raises.
///
/// `on_error="skip"` records the key of any document that exceeds htmlarc's
/// per-document capacity in `skipped` instead of raising — the mode for
/// wild-corpus ingestion, where roughly 1 in 100k real-world pages trips a
/// capacity limit.
///
/// `meta_schema={"name": type, ...}` (types `str`/`int`/`float`/`bool`) declares
/// typed per-document metadata columns stored inside the archive; each add may
/// then carry `meta={...}` (missing fields are null). Readers get them back via
/// `Document.meta`, `Archive.meta_schema`, `Archive.meta_table()` and
/// `scan_table(meta=[...])` — no sidecar file, no join.
#[pyclass(module = "htmlarc")]
pub struct ArchiveBuilder {
    builder: Option<HtmlArchiveBuilder>,
    path: Option<PathBuf>,
    skip_on_error: bool,
    skipped: Vec<String>,
    meta_schema: Option<MetaSchema>,
}

#[pymethods]
impl ArchiveBuilder {
    #[new]
    #[pyo3(signature = (path = None, *, on_error = "raise", meta_schema = None))]
    fn new(
        path: Option<PathBuf>,
        on_error: &str,
        meta_schema: Option<Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let skip_on_error = match on_error {
            "raise" => false,
            "skip" => true,
            other => {
                return Err(PyValueError::new_err(format!(
                    "on_error must be 'raise' or 'skip', got {other:?}"
                )));
            }
        };
        let mut builder = HtmlArchiveBuilder::default();
        let meta_schema = match meta_schema {
            Some(dict) => {
                let schema = meta_schema_from_py(&dict)?;
                builder
                    .set_meta_schema(schema.clone())
                    .map_err(|e| PyValueError::new_err(e.to_string()))?;
                Some(schema)
            }
            None => None,
        };
        Ok(ArchiveBuilder {
            builder: Some(builder),
            path,
            skip_on_error,
            skipped: Vec::new(),
            meta_schema,
        })
    }

    /// Parse `html` and add it under `key`. Raises `ValueError` when the HTML exceeds
    /// htmlarc's per-document capacity — unless the builder was created with
    /// `on_error="skip"`, in which case the document is dropped and its key appended
    /// to `skipped`. With a `meta_schema`, `meta={...}` attaches this document's
    /// metadata row (missing fields are null).
    #[pyo3(signature = (key, html, *, meta = None))]
    fn add(&mut self, key: &str, html: &str, meta: Option<Bound<'_, PyDict>>) -> PyResult<()> {
        let row = self.convert_meta(meta.as_ref())?;
        let builder = self
            .builder
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        match HtmlDoc::parse(html) {
            Ok(doc) => Self::add_parsed(builder, key, doc, row),
            Err(_) if self.skip_on_error => {
                self.skipped.push(key.to_string());
                Ok(())
            }
            Err(e) => Err(PyValueError::new_err(e.to_string())),
        }
    }

    /// Add an already-parsed `Document` under `key` without re-parsing — the path for
    /// crawlers that parse each page anyway (e.g. for link discovery): parse once, query
    /// for links, then store the same `Document`. With a `meta_schema`, `meta={...}`
    /// attaches this document's metadata row (missing fields are null).
    ///
    /// Accepts documents from `parse()`. Documents handed out by an `Archive` are backed
    /// by shared per-bundle storage and can't be re-added directly; raises `TypeError`
    /// for those (round-trip through `add(key, doc.to_html())` instead).
    #[pyo3(signature = (key, doc, *, meta = None))]
    fn add_document(
        &mut self,
        key: &str,
        doc: Bound<'_, Document>,
        meta: Option<Bound<'_, PyDict>>,
    ) -> PyResult<()> {
        let row = self.convert_meta(meta.as_ref())?;
        let builder = self
            .builder
            .as_mut()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        match &doc.get().backing {
            Backing::Parsed(dom) => {
                Self::add_parsed(builder, key, HtmlDoc::from(dom.as_ref().clone()), row)
            }
            Backing::Archived(_) => Err(PyTypeError::new_err(
                "document is archive-backed (its text lives in shared bundle storage); \
                 use add(key, doc.to_html()) to copy it into a new archive",
            )),
        }
    }

    /// Write the archive and consume the builder. `path` may be omitted when it was
    /// given at construction; passing one here overrides the constructor's.
    #[pyo3(signature = (path = None))]
    fn write(&mut self, path: Option<PathBuf>) -> PyResult<()> {
        let path = path.or_else(|| self.path.clone()).ok_or_else(|| {
            PyValueError::new_err("no path: pass write(path) or construct ArchiveBuilder(path)")
        })?;
        let builder = self
            .builder
            .take()
            .ok_or_else(|| PyRuntimeError::new_err("archive already written"))?;
        builder.write_to(path).map_err(archive_err)
    }

    /// Keys of documents dropped by `on_error="skip"`, in add order. Empty when
    /// `on_error="raise"` (the default) or when nothing was dropped.
    #[getter]
    fn skipped(&self) -> Vec<String> {
        self.skipped.clone()
    }

    /// Enter the context manager. Requires a `path` from the constructor — the write
    /// destination for `__exit__`.
    fn __enter__(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<Self>> {
        if slf.borrow(py).path.is_none() {
            return Err(PyValueError::new_err(
                "ArchiveBuilder used as a context manager needs a path: \
                 ArchiveBuilder('out.htmlarc')",
            ));
        }
        Ok(slf.clone_ref(py))
    }

    /// Exit the context manager: write the archive to the constructor's `path` on
    /// clean exit; skip the write when the block raised.
    #[pyo3(signature = (exc_type, exc, tb))]
    fn __exit__(
        &mut self,
        exc_type: Option<Bound<'_, PyAny>>,
        exc: Option<Bound<'_, PyAny>>,
        tb: Option<Bound<'_, PyAny>>,
    ) -> PyResult<bool> {
        let (_, _) = (exc, tb);
        if exc_type.is_none() {
            self.write(None)?;
        }
        Ok(false)
    }
}

impl ArchiveBuilder {
    /// Convert a per-add `meta={...}` dict to a schema-ordered row; `meta` without a declared
    /// schema is an error (it would be silently dropped otherwise).
    fn convert_meta(
        &self,
        meta: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Option<Vec<Option<MetaValue>>>> {
        match (meta, &self.meta_schema) {
            (None, _) => Ok(None),
            (Some(_), None) => Err(PyValueError::new_err(
                "meta given but the builder was constructed without a meta_schema",
            )),
            (Some(dict), Some(schema)) => Ok(Some(meta_row_from_py(schema, dict)?)),
        }
    }

    fn add_parsed(
        builder: &mut HtmlArchiveBuilder,
        key: &str,
        doc: HtmlDoc,
        row: Option<Vec<Option<MetaValue>>>,
    ) -> PyResult<()> {
        match row {
            Some(row) => builder
                .add_html_with_meta(key.to_string(), doc, row)
                .map_err(|e| PyValueError::new_err(e.to_string())),
            None => {
                builder.add_html(key.to_string(), doc);
                Ok(())
            }
        }
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
