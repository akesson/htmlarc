//! A tolerant, parse-free counting pass over one HTML document.
//!
//! Unlike `HtmlDoc::parse`, this never fails on unknown tags/attributes — exactly the
//! documents the probe most needs to measure. It drives the same `html5gum` tokenizer the
//! real parser uses (so raw-text elements, entity decoding, etc. behave the same) and tallies
//! the cardinalities that gate ADR 0002's reference-space constants. Counts are approximate
//! where the real tree builder would differ (e.g. whitespace text nodes), which is fine for
//! sizing ceilings.

use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{BuildHasher, BuildHasherDefault};

use html5gum::Tokenizer;
use html5gum::emitters::callback::{CallbackEmitter, CallbackEvent};
use htmlarc_dom::html::{HtmlAttr, HtmlTag};

/// Attribute *values* that ADR 0002 routes to Lane A (exact-searched), alongside `id`.
const SEARCHED_ATTRS: [&str; 5] = ["lang", "rel", "type", "role", "name"];

/// Per-document cardinalities.
pub(crate) struct DocStats {
    /// Approximate node count (start tags + text + comments + doctype).
    pub nodes: u32,
    /// Maximum element nesting depth.
    pub max_depth: u32,
    /// Total list entries the unified store would hold: every class-token occurrence plus
    /// every non-class attribute occurrence.
    pub list_entries: u32,
    /// Distinct `(attr-name, value)` pairs (the future attribute entries table; excludes
    /// `class`, which becomes a bare symbol list).
    pub distinct_pairs: u32,
    /// Distinct tag names not in the `HtmlTag` enum (SVG/MathML children, custom elements):
    /// the per-document extended-tag vocabulary, gating `EXT_BASE`.
    pub ext_tag_names: u32,
    /// Distinct attribute names not in the `HtmlAttr` enum (includes full `data-*` names).
    pub ext_attr_names: u32,
    /// Size of the per-document Lane A symbol set: class tokens ∪ ids ∪ searched values ∪
    /// extended tag/attr names. Gates `LOCAL_CAP`.
    pub sym_union: u32,
    /// The Lane A symbol strings of this document (for the per-bundle shared-dict simulation).
    pub lane_a: Vec<String>,
    /// The Lane B bytes of this document — text/comment payload and content-attribute values
    /// (everything routed to the compress lane). Empty unless compression measurement is on.
    pub lane_b: Vec<u8>,
}

type FixedHasher = BuildHasherDefault<DefaultHasher>;

#[derive(Default)]
struct Counter {
    nodes: u32,
    depth: u32,
    max_depth: u32,
    list_entries: u32,

    classes: HashSet<String>,
    ids: HashSet<String>,
    searched: HashSet<String>,
    ext_tags: HashSet<String>,
    ext_attrs: HashSet<String>,
    pair_hashes: HashSet<u64>,
    hasher: FixedHasher,

    // Lane B (compress lane) bytes; only collected when `capture_b` is set.
    capture_b: bool,
    lane_b: Vec<u8>,

    // Transient per-tag state.
    cur_attr: Option<String>,
    cur_tag_void: bool,
}

fn lossy_lower(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).to_ascii_lowercase()
}

impl Counter {
    fn open_start_tag(&mut self, name: &[u8]) {
        self.flush_attr(None); // a previous tag's trailing valueless attribute
        self.nodes += 1;
        let name = lossy_lower(name);
        match HtmlTag::try_from(name.as_str()) {
            Ok(tag) => self.cur_tag_void = tag.is_void_element(),
            Err(_) => {
                self.cur_tag_void = false;
                self.ext_tags.insert(name);
            }
        }
    }

    fn attribute_name(&mut self, name: &[u8]) {
        self.flush_attr(None);
        self.cur_attr = Some(String::from_utf8_lossy(name).into_owned());
    }

    fn attribute_value(&mut self, value: &[u8]) {
        let value = String::from_utf8_lossy(value).into_owned();
        self.flush_attr(Some(value));
    }

    /// Finalize the pending attribute with `value` (or empty if `None`).
    fn flush_attr(&mut self, value: Option<String>) {
        let Some(name) = self.cur_attr.take() else {
            return;
        };
        let value = value.unwrap_or_default();
        let lname = name.to_ascii_lowercase();

        if lname == "class" {
            // Each whitespace-separated token is one symbol and one list entry.
            for token in value.split_ascii_whitespace() {
                self.classes.insert(token.to_string());
                self.list_entries += 1;
            }
            return;
        }

        self.list_entries += 1;
        self.pair_hashes
            .insert(self.hasher.hash_one((&lname, &value)));
        if HtmlAttr::try_from(lname.as_str()).is_err() {
            self.ext_attrs.insert(lname.clone());
        }
        if lname == "id" {
            self.ids.insert(value);
        } else if SEARCHED_ATTRS.contains(&lname.as_str()) {
            self.searched.insert(value);
        } else if self.capture_b {
            // A content attribute value (href/title/src/alt/data-*/…) → Lane B.
            self.lane_b.extend_from_slice(value.as_bytes());
            self.lane_b.push(b'\n');
        }
    }

    fn close_start_tag(&mut self, self_closing: bool) {
        self.flush_attr(None);
        if !self_closing && !self.cur_tag_void {
            // Naive nesting: this counts raw open/close tags with no implied-end-tag or
            // auto-close, so it is an upper bound that overcounts the real tree-builder
            // stack (which collapses `<p>`/`<li>`/`<td>` soup). Use it as a ceiling probe,
            // not as the effective depth the parser actually reaches.
            self.depth += 1;
            self.max_depth = self.max_depth.max(self.depth);
        }
    }

    fn end_tag(&mut self) {
        self.flush_attr(None);
        self.depth = self.depth.saturating_sub(1);
    }

    fn handle(&mut self, event: CallbackEvent<'_>) {
        match event {
            CallbackEvent::OpenStartTag { name } => self.open_start_tag(name),
            CallbackEvent::AttributeName { name } => self.attribute_name(name),
            CallbackEvent::AttributeValue { value } => self.attribute_value(value),
            CallbackEvent::CloseStartTag { self_closing } => self.close_start_tag(self_closing),
            CallbackEvent::EndTag { .. } => self.end_tag(),
            CallbackEvent::String { value } => {
                if !value.is_empty() {
                    self.nodes += 1;
                    if self.capture_b {
                        self.lane_b.extend_from_slice(value);
                        self.lane_b.push(b'\n');
                    }
                }
            }
            CallbackEvent::Comment { value } => {
                self.nodes += 1;
                if self.capture_b {
                    self.lane_b.extend_from_slice(value);
                    self.lane_b.push(b'\n');
                }
            }
            CallbackEvent::Doctype { .. } => self.nodes += 1,
            CallbackEvent::Error(_) => {}
        }
    }

    fn finish(mut self) -> DocStats {
        let ext_tag_names = self.ext_tags.len() as u32;
        let ext_attr_names = self.ext_attrs.len() as u32;
        let distinct_pairs = self.pair_hashes.len() as u32;

        // The Lane A symbol set is the union of every searched/identity string.
        let mut union: HashSet<String> = HashSet::new();
        for set in [
            &mut self.classes,
            &mut self.ids,
            &mut self.searched,
            &mut self.ext_tags,
            &mut self.ext_attrs,
        ] {
            union.extend(set.drain());
        }

        DocStats {
            nodes: self.nodes,
            max_depth: self.max_depth,
            list_entries: self.list_entries,
            distinct_pairs,
            ext_tag_names,
            ext_attr_names,
            sym_union: union.len() as u32,
            lane_a: union.into_iter().collect(),
            lane_b: std::mem::take(&mut self.lane_b),
        }
    }
}

/// Count one document's cardinalities with a tolerant tokenizer pass. When `capture_b` is
/// set, also collect the document's Lane B bytes (text + content-attribute values).
pub(crate) fn count_doc(html: &str, capture_b: bool) -> DocStats {
    let mut counter = Counter {
        capture_b,
        ..Default::default()
    };
    {
        let emitter: CallbackEmitter<_, (), ()> =
            CallbackEmitter::new(|event: CallbackEvent<'_>, _span| {
                counter.handle(event);
                Option::<()>::None
            });
        for _ in Tokenizer::new_with_emitter(html, emitter) {}
    }
    counter.finish()
}

#[cfg(test)]
mod tests {
    use super::count_doc;

    #[test]
    fn counts_extended_tags_attrs_and_symbols() {
        // `svg`/`math` are in the HtmlTag enum; their children and the custom elements are
        // the extended-tag vocabulary. `class` is an enum attr but its tokens are symbols.
        let html = r#"<html><body>
            <svg viewBox="0 0 1 1"><path d="M0 0"/><g><circle cx="1"/></g></svg>
            <math><mrow><mi>x</mi></mrow></math>
            <my-widget data-foo="1" data-bar="2" class="a b a" id="w"></my-widget>
            <custom-thing aria-zonk="x"></custom-thing>
        </body></html>"#;
        let s = count_doc(html, true);

        // path, g, circle, mrow, mi, my-widget, custom-thing  (svg/math are enum tags).
        assert_eq!(s.ext_tag_names, 7);
        // viewbox, d, cx, data-foo, data-bar, aria-zonk  (class/id are enum attrs).
        assert_eq!(s.ext_attr_names, 6);
        // 3 class-token occurrences ("a b a") + 7 non-class attrs.
        assert_eq!(s.list_entries, 10);
        // classes{a,b}=2, ids{w}=1, ext_tags=7, ext_attrs=6 → 16.
        assert_eq!(s.sym_union, 16);
        // capture_b=true collected text + content-attr values into Lane B.
        assert!(!s.lane_b.is_empty());
    }

    #[test]
    fn plain_document_has_no_extended_names() {
        let s = count_doc("<div class='x'><p>hi</p><a href='/y'>link</a></div>", false);
        assert_eq!(s.ext_tag_names, 0);
        assert_eq!(s.ext_attr_names, 0);
        assert!(s.max_depth >= 2);
        // capture_b=false leaves Lane B empty.
        assert!(s.lane_b.is_empty());
    }
}
