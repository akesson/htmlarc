//! HTML tokenization driver built on the [`html5gum`] tokenizer.
//!
//! `html5gum` is a spec-compliant *tokenizer* (it implements WHATWG 13.2.5, decodes
//! character references in text / RCDATA / attribute values, and leaves RAWTEXT —
//! `<script>`, `<style>` — verbatim) but ships no tree builder. This module is the thin
//! glue that turns its token stream into calls on htmlarc's [`DomStack`] tree builder,
//! reproducing the behaviour of the previous hand-rolled parser:
//!
//! - unrecognised *tags* parse as extended (custom) elements via a per-document vocab (ADR
//!   0002 §4); an unrecognised *attribute* name is likewise kept as an extended name (ADR
//!   0002 §3), so `data-*`, arbitrary attributes, and custom elements all parse;
//! - `<svg>` / `<math>` subtrees are stored as ordinary (extended) elements (ADR 0002 §5);
//!   while inside one, the raw-text state switch is suppressed and `<![CDATA[…]]>` is kept as
//!   character data (see [`RawTextEmitter`]);
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

use super::dom::{DomStack, TagName};

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
/// [`raw_next_state`] instead of html5gum's broader built-in heuristic, and to approximate
/// the WHATWG "foreign content" rules an extraction archive needs (ADR 0002 §5). Everything
/// except the post-tag state decision and the foreign-content flag is forwarded to the inner
/// emitter.
struct RawTextEmitter<E> {
    inner: E,
    /// Name of the tag currently being built (lowercased by the tokenizer).
    tag_name: Vec<u8>,
    /// Whether that tag is a start tag (only start tags trigger a raw-text switch).
    is_start: bool,
    /// Set when the current tag is self-closing (`<svg/>`), so a self-closing foreign root is
    /// not counted as opening a foreign subtree.
    self_closing: bool,
    /// Open-element depth of `svg`/`math` subtrees. While `> 0` we are in foreign content:
    /// the raw-text state switch is suppressed (so `<style>`/`<title>`/`<script>` children
    /// parse as ordinary markup) and `<![CDATA[…]]>` is tokenized as character data rather
    /// than a bogus comment. Name-based, not a full namespace stack — a deliberate
    /// approximation for a fault-tolerant extractor (an unclosed `<svg>` keeps the flag set
    /// until EOF).
    foreign_depth: u32,
}

impl<E: Emitter> RawTextEmitter<E> {
    fn new(inner: E) -> Self {
        Self {
            inner,
            tag_name: Vec::new(),
            is_start: false,
            self_closing: false,
            foreign_depth: 0,
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
        self.self_closing = false;
        self.inner.init_start_tag();
    }

    fn init_end_tag(&mut self) {
        self.tag_name.clear();
        self.is_start = false;
        self.self_closing = false;
        self.inner.init_end_tag();
    }

    fn push_tag_name(&mut self, s: &[u8]) {
        self.tag_name.extend_from_slice(s);
        self.inner.push_tag_name(s);
    }

    fn set_self_closing(&mut self) {
        self.self_closing = true;
        self.inner.set_self_closing();
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        // Forward to emit the tag's events, then maintain the foreign-content depth and
        // override the state decision.
        let _ = self.inner.emit_current_tag();
        let is_foreign = matches!(self.tag_name.as_slice(), b"svg" | b"math");
        if self.is_start {
            if is_foreign && !self.self_closing {
                self.foreign_depth += 1;
            }
            // Inside foreign content nothing switches to RAWTEXT/RCDATA — children parse as
            // ordinary markup (ADR 0002 §5).
            if self.foreign_depth == 0 {
                raw_next_state(&self.tag_name)
            } else {
                None
            }
        } else {
            if is_foreign {
                self.foreign_depth = self.foreign_depth.saturating_sub(1);
            }
            None
        }
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&mut self) -> bool {
        // Drives html5gum's markup-declaration path: `true` makes `<![CDATA[…]]>` a CDATA
        // section (character data) instead of a bogus comment, matching browser behaviour
        // inside svg/math.
        self.foreign_depth > 0
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
        foreign_depth: 0,
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
    /// A recognised HTML element (including `svg`/`math`, now stored like any other — ADR
    /// 0002 §5).
    Std(HtmlTag),
    /// An extended (custom/unknown) element — the verbatim name, kept owned because the
    /// html5gum event slice is transient and the tag commits lazily (ADR 0002 §4).
    Ext(String),
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
    /// Open-element depth of `svg`/`math` subtrees, mirrored from the emitter's foreign-content
    /// tracking (`RawTextEmitter::foreign_depth`). The pop decision in
    /// [`close_start_tag`](Self::close_start_tag) needs it to tell a foreign `<path/>` (the
    /// self-closing flag is honored — childless) from an ordinary `<div/>` (the flag is ignored
    /// — stays open). svg/math children are stored as `extended`, indistinguishable from a
    /// non-foreign custom element by tag alone, so depth is the only available signal (ADR 0003).
    foreign_depth: u32,
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

    /// Materialise the deferred start tag: push the element. A no-op once already materialised.
    fn commit_start(&mut self) {
        match self.start.take() {
            Some(StartTag::Std(tag)) => {
                self.dom.push_tag(TagName::Std(tag));
                self.current = Some(tag);
            }
            Some(StartTag::Ext(name)) => {
                self.dom.push_tag(TagName::Ext(&name));
                self.current = Some(HtmlTag::extended);
            }
            None => {}
        }
    }

    fn open_start_tag(&mut self, name: &[u8]) {
        let name = String::from_utf8_lossy(name);
        // Unknown names are no longer a hard error: they parse as extended (custom) tags
        // (ADR 0002 §4). `svg`/`math` are ordinary recognised elements now (ADR 0002 §5).
        self.start = Some(match TagName::parse(&name) {
            TagName::Std(tag) => StartTag::Std(tag),
            TagName::Ext(_) => StartTag::Ext(name.to_string()),
        });
    }

    fn attribute_name(&mut self, name: &[u8]) {
        // The text preceding this tag has now been flushed; materialise the tag.
        self.commit_start();
        // A new name means the previous attribute (if any) had no value.
        self.flush_attr("");
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
        self.flush_attr(""); // a trailing valueless attribute
        let Some(tag) = self.current.take() else {
            return;
        };
        // Is this element foreign — svg/math itself, or anything inside an open svg/math
        // subtree? Foreign children are stored as `extended`, so the depth, not the tag, is the
        // signal (see `foreign_depth`).
        let in_foreign = tag.is_foreign_element() || self.foreign_depth > 0;
        if tag.is_void_element() || (self_closing && in_foreign) {
            // A void element has no end tag; a self-closing foreign element (`<path/>`) is
            // childless. Either way it was pushed an instant ago, so pop it directly — an
            // identity check would be vacuous (and impossible for an extended tag, whose kind
            // alone is `extended`).
            self.dom._pop_tag();
        } else if tag.is_foreign_element() {
            // A non-self-closing `<svg>`/`<math>` opens a foreign subtree; track its depth so
            // the self-closing decision above stays correct for the descendants within it.
            self.foreign_depth += 1;
        }
        // An ordinary HTML element or non-foreign custom element carrying a stray self-closing
        // slash (`<div/>`, `<x-y/>`) is intentionally NOT popped: HTML5 ignores the slash on
        // these and keeps the element open, so a later `</div>` matches it instead of orphaning
        // the whole document — the dominant structural-failure bucket the converter used to
        // drop (ADR 0003). The element auto-closes at its real end tag, EOF, or an implied end.
    }

    fn end_tag(&mut self, name: &[u8]) {
        let name = String::from_utf8_lossy(name);
        // Unknown end-tag names parse as extended tags rather than erroring (ADR 0002 §4); a
        // genuine mismatch is still caught by `pop_tag`'s identity check.
        let tag = TagName::parse(&name);
        // Leaving an `<svg>`/`<math>` subtree: mirror the emitter's foreign-depth decrement so
        // the self-closing decision in `close_start_tag` reverts once we are back in HTML.
        if let TagName::Std(t) = &tag
            && t.is_foreign_element()
        {
            self.foreign_depth = self.foreign_depth.saturating_sub(1);
        }
        self.pop(tag);
    }

    fn text(&mut self, value: &[u8]) {
        // Note: a deferred start tag is intentionally *not* committed here — text that
        // arrives while one is pending precedes it and belongs to the current parent.
        if value.is_empty() {
            return;
        }
        let value = String::from_utf8_lossy(value);
        self.dom.add_text_tag(HtmlTag::sys_text, &value);
    }

    fn comment(&mut self, value: &[u8]) {
        let value = String::from_utf8_lossy(value);
        self.dom.add_text_tag(HtmlTag::sys_comment, &value);
    }

    fn doctype(&mut self) {
        self.dom.push_tag(TagName::Std(HtmlTag::DOCTYPE));
        self.dom.add_attribute(AttrName::Std(HtmlAttr::html), "");
        // Just pushed, so pop it directly (see `close_start_tag`).
        self.dom._pop_tag();
    }

    fn pop(&mut self, name: TagName<'_>) {
        if let Err(err) = self.dom.pop_tag(name) {
            self.error = Some(err);
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
