use crate::{
    dom::DomInner,
    fmt::{fmt_buf::FmtBuf, spaces::Spaces},
};

use super::pretty::Inline;

pub trait CommonFormatting {
    fn dom_and_buf(&mut self) -> (&DomInner, &mut FmtBuf);

    fn add_doctype(&mut self, index: u16) {
        let (dom, buf) = self.dom_and_buf();
        buf.push_str("<!DOCTYPE");
        if let Some(list_index) = dom.nodes.attr_list_index(index) {
            buf.add_attrs(dom.attrs.list_at(list_index));
        }
        buf.push('>');
    }

    fn push_attributes(&mut self, index: u16) {
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

    fn push_trimmed_text(&mut self, index: u16, inline: Inline) -> bool {
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
        buf.push_str(&cleaned);
        true
    }
}
