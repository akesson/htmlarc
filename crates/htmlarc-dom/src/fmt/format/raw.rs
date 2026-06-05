use crate::{
    dom::{DomView, NodeIndex},
    fmt::{
        fmt_buf::FmtBuf,
        iter::{TagIter, TagStage},
    },
    html::HtmlTag,
};

use super::common::CommonFormatting;

pub struct RawFormat<'dom> {
    dom: DomView<'dom>,
    buf: FmtBuf,
}
impl<'dom> CommonFormatting<'dom> for RawFormat<'dom> {
    fn dom_and_buf(&mut self) -> (DomView<'dom>, &mut FmtBuf) {
        (self.dom, &mut self.buf)
    }
}

impl<'dom> RawFormat<'dom> {
    pub fn new(dom: DomView<'dom>) -> Self {
        Self {
            dom,
            buf: Default::default(),
        }
    }

    pub fn html(mut self, index: NodeIndex) -> String {
        for elem in TagIter::new(self.dom, index) {
            let tag = self.dom.nodes.tag(elem.index);

            match elem.stage {
                TagStage::Open => match tag {
                    HtmlTag::DOCTYPE => self.add_doctype(elem.index),
                    HtmlTag::sys_text => self.buf.push_str(self.dom.string_at(elem.index)),
                    HtmlTag::sys_comment => self.buf.add_comment(self.dom.string_at(elem.index)),
                    _ => self.add_start_tag(elem.index, tag),
                },
                TagStage::Close => self.add_close_tag(elem.index),
            }
        }
        self.buf.inner()
    }

    fn add_start_tag(&mut self, index: NodeIndex, tag: HtmlTag) {
        self.buf.push('<');
        self.buf.push_str(tag.into());
        self.push_attributes(index);
        if tag.auto_close() {
            self.buf.push(' ');
            self.buf.push('/');
        }
        self.buf.push('>');
    }

    // fn self_close(&self, index: u16, tag: HtmlTag) -> bool {
    //     let has_children = self.dom.nodes.first_child_index(index).is_some();
    //     (tag.is_foreign_element() && !has_children) || tag.is_void_element()
    // }

    fn add_close_tag(&mut self, index: NodeIndex) {
        let tag = self.dom.nodes.tag(index);

        if tag.no_close() {
            return;
        }
        self.buf.push_str("</");
        self.buf.push_str(self.dom.nodes.tag(index).into());
        self.buf.push('>');
    }
}
