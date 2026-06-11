use crate::{
    dom::{DomView, NodeIndex},
    entities,
    fmt::{fmt_buf::FmtBuf, spaces::Spaces},
};

use super::pretty::Inline;

pub trait CommonFormatting<'dom> {
    /// The (Copy) document view and the output buffer. The view carries the document
    /// data lifetime `'dom` (not the `&mut self` borrow) so the borrowed list
    /// iterators it yields don't clash with mutating `buf`.
    fn dom_and_buf(&mut self) -> (DomView<'dom>, &mut FmtBuf);

    fn add_doctype(&mut self, index: NodeIndex) {
        let (dom, buf) = self.dom_and_buf();
        buf.push_str("<!DOCTYPE");
        if let Some(list_index) = dom.nodes.attr_list_index(index) {
            buf.add_attrs(dom.attr_list_at(list_index));
        }
        buf.push('>');
    }

    fn push_attributes(&mut self, index: NodeIndex) {
        let (dom, buf) = self.dom_and_buf();
        if let Some(list_index) = dom.nodes.class_list_index(index) {
            buf.add_classes(dom.class_list_at(list_index));
        }
        // Standard, `data-*`, and unknown attributes share one run, rendered in source
        // order (ADR 0002 §3) — no more class-then-data-then-std split.
        if let Some(list_index) = dom.nodes.attr_list_index(index) {
            buf.add_attrs(dom.attr_list_at(list_index));
        }
    }

    fn push_trimmed_text(&mut self, index: NodeIndex, inline: Inline, rawtext: bool) -> bool {
        let (dom, buf) = self.dom_and_buf();

        let text = dom.string_at(index);

        let cleaned = Spaces::remove_formatting(text);

        if cleaned.is_empty() {
            return false;
        }
        // let spaces = Spaces::count(&cleaned);

        if inline == Inline::Start {
            buf.newline();
        }
        // RAWTEXT (script/style) is stored verbatim and must not be entity-encoded — doing
        // so would corrupt `&`/`<`/`>` in JS/CSS. Every other text node was decoded on
        // ingest and is re-encoded here. Mirrors the guard in `RawFormat`.
        if rawtext {
            buf.push_str(&cleaned);
        } else {
            buf.push_str(&entities::encode_text(&cleaned));
        }
        true
    }
}
