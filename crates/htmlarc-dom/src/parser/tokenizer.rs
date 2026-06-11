//! HTML tokenization driver built on the [`html5gum`] tokenizer.
//!
//! `html5gum` is a spec-compliant *tokenizer* (it implements WHATWG 13.2.5, decodes
//! character references in text / RCDATA / attribute values, and leaves RAWTEXT —
//! `<script>`, `<style>` — verbatim) but ships no tree builder. This module is the thin
//! glue that turns its token stream into calls on htmlarc's [`DomStack`] tree builder,
//! reproducing the behaviour of the previous hand-rolled parser:
//!
//! - unrecognised *tags* are a hard error (element-tag tolerance is ADR 0002 PR 4); an
//!   unrecognised *attribute* name is kept as an extended name (ADR 0002 §3), so `data-*`
//!   and arbitrary attributes both parse;
//! - `<svg>` / `<math>` subtrees are skipped wholesale;
//! - void / self-closing elements are popped immediately;
//! - a `<!DOCTYPE …>` is stored as a fixed `DOCTYPE` node with a single `html` attribute.
//!
//! We drive html5gum's *callback* emitter, which streams `AttributeName`/`AttributeValue`
//! events in source order — so attribute order is preserved (its default emitter sorts them
//! into a `BTreeMap`). The same emitter exposes byte-offset spans, the foundation for future
//! `-v` error reporting (currently ignored).
//!
//! ## Deferred start tags
//!
//! The callback emitter flushes buffered character data *lazily* (before an attribute name,
//! a comment, or EOF — but **not** before `OpenStartTag`). So the text that textually
//! precedes a tag-with-attributes arrives *after* that tag's `OpenStartTag`, between it and
//! the first `AttributeName`. To attach such text to the correct parent, we do not push an
//! element at `OpenStartTag`; we defer it until the tag is "materialised" — at its first
//! attribute or its `CloseStartTag` — by which point any preceding text has been flushed.
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use html5gum::{Emitter, ForwardingEmitter, State, Tokenizer};

use crate::html::{HtmlAttr, HtmlTag};
use crate::stores::AttrName;
use crate::{HtmlParseError, HtmlParseResult};

use super::dom::DomStack;

/// The next tokenizer state to switch into after a start tag, restricted to the elements the
/// previous hand-rolled parser treated as raw: `script`/`style` (verbatim) and
/// `title`/`textarea` (RCDATA, entity-decoded).
///
/// This deliberately differs from html5gum's [`html5gum::naive_next_state`], which also
/// rawtexts `noscript`/`iframe`/`xmp`/`noembed`/`noframe`/`plaintext` — turning their inner
/// markup into opaque text. For an extraction archive we want those parsed as normal HTML so
/// e.g. the `<img>` fallbacks inside `<noscript>` stay queryable.
fn raw_next_state(tag_name: &[u8]) -> Option<State> {
    match tag_name {
        b"title" | b"textarea" => Some(State::RcData),
        b"script" => Some(State::ScriptData),
        b"style" => Some(State::RawText),
        _ => None,
    }
}

/// Wraps an [`Emitter`] to drive the tokenizer/tree-builder state feedback via
/// [`raw_next_state`] instead of html5gum's broader built-in heuristic. Everything except the
/// post-tag state decision is forwarded to the inner emitter.
struct RawTextEmitter<E> {
    inner: E,
    /// Name of the tag currently being built (lowercased by the tokenizer).
    tag_name: Vec<u8>,
    /// Whether that tag is a start tag (only start tags trigger a raw-text switch).
    is_start: bool,
}

impl<E: Emitter> RawTextEmitter<E> {
    fn new(inner: E) -> Self {
        Self {
            inner,
            tag_name: Vec::new(),
            is_start: false,
        }
    }
}

impl<E: Emitter> ForwardingEmitter for RawTextEmitter<E> {
    type Token = E::Token;

    fn inner(&mut self) -> &mut impl Emitter<Token = Self::Token> {
        &mut self.inner
    }

    fn init_start_tag(&mut self) {
        self.tag_name.clear();
        self.is_start = true;
        self.inner.init_start_tag();
    }

    fn init_end_tag(&mut self) {
        self.tag_name.clear();
        self.is_start = false;
        self.inner.init_end_tag();
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.tag_name.extend_from_slice(s);
        self.inner.push_tag_name(s);
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        // Forward to emit the tag's events, then override the state decision.
        let _ = self.inner.emit_current_tag();
        if self.is_start {
            raw_next_state(&self.tag_name)
        } else {
            None
        }
    }
}

/// Tokenize `input` and drive `dom` — any [`DomStack`], i.e. the real builder or the test
/// DOM. Returns the first parse error encountered (mimicking the old hard-failing parser).
pub(crate) fn parse_into<D: DomStack>(input: &str, dom: &mut D) -> HtmlParseResult<()> {
    let mut driver = Driver {
        dom,
        error: None,
        start: None,
        attr: None,
        current: None,
        skip: None,
        skip_awaiting_close: false,
    };

    {
        // `T = ()` (the callback yields no tokens) and `S = ()` (no span tracking); pin both
        // since an always-`None` callback can't infer them.
        let callback: CallbackEmitter<_, (), ()> =
            CallbackEmitter::new(|event: CallbackEvent<'_>, _span| {
                driver.handle(event);
                // The callback never yields a token; all work is side effects on `driver`.
                Option::<()>::None
            });
        // `RawTextEmitter` approximates the tokenizer/tree-builder feedback loop (so
        // `<script>`/`<style>`/`<title>`/`<textarea>` tokenize correctly) without us tracking
        // open tags, and limits the raw-text set to those four (see `raw_next_state`).
        let emitter = RawTextEmitter::new(callback);

        // Because the callback always returns `None`, a single `next()` drives the whole
        // input; the loop just runs the tokenizer to completion.
        for _ in Tokenizer::new_with_emitter(input, emitter) {}
    }

    match driver.error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

/// A start tag seen via `OpenStartTag` but not yet materialised — see [`Driver::start`].
enum StartTag {
    Element(HtmlTag),
    /// `svg`/`math`, whose subtree is dropped wholesale.
    Foreign(HtmlTag),
}

/// An attribute name that has been seen and is awaiting its (optional) value.
enum PendingName {
    Std(HtmlAttr),
    /// An extended name (any `data-*` or otherwise unrecognised attribute), kept verbatim
    /// — the full name, including any `data-` prefix.
    Ext(String),
}

struct Driver<'d, D: DomStack> {
    dom: &'d mut D,
    error: Option<HtmlParseError>,
    /// A start tag accumulated since its `OpenStartTag`, materialised lazily (see module
    /// docs). `None` when not inside a start tag.
    start: Option<StartTag>,
    /// An attribute name awaiting its value.
    attr: Option<PendingName>,
    /// The element currently open (materialised), for the void/self-closing pop decision.
    current: Option<HtmlTag>,
    /// The foreign element (`svg`/`math`) whose subtree is currently being dropped.
    skip: Option<HtmlTag>,
    /// True between a foreign element being materialised and its own `CloseStartTag`, to
    /// tell that close apart from a nested child's while skipping.
    skip_awaiting_close: bool,
}

impl<D: DomStack> Driver<'_, D> {
    fn handle(&mut self, event: CallbackEvent<'_>) {
        if self.error.is_some() {
            return;
        }
        match event {
            CallbackEvent::OpenStartTag { name } => self.open_start_tag(name),
            CallbackEvent::AttributeName { name } => self.attribute_name(name),
            CallbackEvent::AttributeValue { value } => self.attribute_value(value),
            CallbackEvent::CloseStartTag { self_closing } => self.close_start_tag(self_closing),
            CallbackEvent::EndTag { name } => self.end_tag(name),
            CallbackEvent::String { value } => self.text(value),
            CallbackEvent::Comment { value } => self.comment(value),
            CallbackEvent::Doctype { .. } => self.doctype(),
            // Parse errors are best-effort and ignored (future `-v` will surface them).
            CallbackEvent::Error(_) => {}
        }
    }

    /// Materialise the deferred start tag: push a normal element, or enter skip mode for a
    /// foreign one. A no-op once already materialised.
    fn commit_start(&mut self) {
        match self.start.take() {
            Some(StartTag::Element(tag)) => {
                self.dom.push_tag(tag);
                self.current = Some(tag);
            }
            Some(StartTag::Foreign(tag)) => {
                self.skip = Some(tag);
                self.skip_awaiting_close = true;
            }
            None => {}
        }
    }

    fn open_start_tag(&mut self, name: &[u8]) {
        if self.skip.is_some() {
            return; // a tag nested inside a skipped foreign subtree
        }
        let name = String::from_utf8_lossy(name);
        match HtmlTag::try_from(name.as_ref()) {
            Ok(tag @ (HtmlTag::svg | HtmlTag::math)) => self.start = Some(StartTag::Foreign(tag)),
            Ok(tag) => self.start = Some(StartTag::Element(tag)),
            Err(_) => self.set_error(format!("Not a valid tag: '{name}'")),
        }
    }

    fn attribute_name(&mut self, name: &[u8]) {
        // The text preceding this tag has now been flushed; materialise the tag.
        self.commit_start();
        // A new name means the previous attribute (if any) had no value.
        self.flush_attr("");
        if self.skip.is_some() {
            return;
        }
        let name = String::from_utf8_lossy(name);
        // Mimic the old parser, whose attribute loop only began a name on an ASCII
        // alphanumeric: stray punctuation (e.g. the doubled quote in `accesskey="f"">`)
        // is skipped as junk rather than treated as an unknown attribute.
        if !name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        {
            return;
        }
        self.attr = Some(match HtmlAttr::try_from(name.as_ref()) {
            Ok(a) => PendingName::Std(a),
            // Any other name — `data-*` or otherwise unrecognised — is kept verbatim as an
            // extended name (the full string, no `data-` stripping). Unknown names are no
            // longer a parse error (ADR 0002 §3).
            Err(_) => PendingName::Ext(name.into_owned()),
        });
    }

    fn attribute_value(&mut self, value: &[u8]) {
        if self.skip.is_some() {
            return;
        }
        let value = String::from_utf8_lossy(value);
        self.flush_attr(&value);
    }

    fn flush_attr(&mut self, value: &str) {
        match self.attr.take() {
            Some(PendingName::Std(a)) => self.dom.add_attribute(AttrName::Std(a), value),
            Some(PendingName::Ext(name)) => self.dom.add_attribute(AttrName::Ext(&name), value),
            None => {}
        }
    }

    fn close_start_tag(&mut self, self_closing: bool) {
        self.commit_start();
        if self.skip.is_some() {
            if self.skip_awaiting_close {
                // The foreign element's own close.
                self.skip_awaiting_close = false;
                if self_closing {
                    self.skip = None; // `<svg/>`: empty, nothing to drop
                }
            }
            return; // otherwise a nested child's close while skipping
        }
        self.flush_attr(""); // a trailing valueless attribute
        if let Some(tag) = self.current.take()
            && (self_closing || tag.is_void_element())
        {
            self.pop(tag);
        }
    }

    fn end_tag(&mut self, name: &[u8]) {
        let name = String::from_utf8_lossy(name);
        if let Some(skip_tag) = self.skip {
            if name.as_ref() == skip_tag.as_str() {
                self.skip = None;
                self.skip_awaiting_close = false;
            }
            return;
        }
        match HtmlTag::try_from(name.as_ref()) {
            Ok(tag) => self.pop(tag),
            Err(_) => self.set_error(format!("Not a valid tag: '{name}'")),
        }
    }

    fn text(&mut self, value: &[u8]) {
        // Note: a deferred start tag is intentionally *not* committed here — text that
        // arrives while one is pending precedes it and belongs to the current parent.
        if self.skip.is_some() || value.is_empty() {
            return;
        }
        let value = String::from_utf8_lossy(value);
        self.dom.add_text_tag(HtmlTag::sys_text, &value);
    }

    fn comment(&mut self, value: &[u8]) {
        if self.skip.is_some() {
            return;
        }
        let value = String::from_utf8_lossy(value);
        self.dom.add_text_tag(HtmlTag::sys_comment, &value);
    }

    fn doctype(&mut self) {
        if self.skip.is_some() {
            return;
        }
        self.dom.push_tag(HtmlTag::DOCTYPE);
        self.dom.add_attribute(AttrName::Std(HtmlAttr::html), "");
        self.pop(HtmlTag::DOCTYPE);
    }

    fn pop(&mut self, tag: HtmlTag) {
        if let Err(err) = self.dom.pop_tag(tag) {
            self.error = Some(err);
        }
    }

    fn set_error(&mut self, message: String) {
        if self.error.is_none() {
            self.error = Some(HtmlParseError::new(message));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::fmt::HtmlFormat;
    use crate::html::HtmlDoc;

    fn rt(html: &str) -> String {
        HtmlDoc::parse(html).unwrap().to_html(HtmlFormat::Raw)
    }

    #[test]
    fn data_attribute_key_with_embedded_data_prefix_survives_round_trip() {
        // Regression: only the *leading* `data-` forms the prefix; the remainder is the
        // key verbatim. A key that itself contains `data-` is legal HTML and must survive
        // parse -> serialize. The previous `str::replace("data-", "")` stripped every
        // occurrence, so `data-data-toggle` collapsed to `data-toggle` and `data-x-data-y`
        // to `data-x-y`.
        for s in [
            r#"<div data-data-toggle="x"></div>"#,
            r#"<div data-x-data-y="1"></div>"#,
            r#"<div data-mw="interface"></div>"#, // ordinary single-prefix key still works
        ] {
            assert_eq!(rt(s), s, "data-attribute key must round-trip: {s:?}");
        }
    }
}
