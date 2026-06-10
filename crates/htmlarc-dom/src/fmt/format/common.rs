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
            buf.add_attrs(dom.attrs.list_at(list_index));
        }
        buf.push('>');
    }

    fn push_attributes(&mut self, index: NodeIndex) {
        let (dom, buf) = self.dom_and_buf();
        if let Some(list_index) = dom.nodes.class_list_index(index) {
            buf.add_classes(dom.classes.list_at(list_index));
        }
        if let Some(data_list_index) = dom.nodes.data_attr_list_index(index) {
            buf.add_data_attrs(dom.dataattrs.list_at(data_list_index));
        }
        if let Some(list_index) = dom.nodes.attr_list_index(index) {
            buf.add_attrs(dom.attrs.list_at(list_index));
        }
    }

    fn push_trimmed_text(&mut self, index: NodeIndex, inline: Inline) -> bool {
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
        buf.push_str(&entities::encode_text(&cleaned));
        true
    }
}
