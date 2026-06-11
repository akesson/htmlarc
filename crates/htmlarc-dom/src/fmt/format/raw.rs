use crate::{
    dom::{DomView, NodeIndex},
    entities,
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
        // RAWTEXT (script/style) content is stored verbatim and must NOT be entity-encoded;
        // every other text node was decoded on ingest and is re-encoded here.
        let mut rawtext_depth = 0u32;
        for elem in TagIter::new(self.dom, index) {
            let tag = self.dom.nodes.tag(elem.index);

            match elem.stage {
                TagStage::Open => match tag {
                    HtmlTag::DOCTYPE => self.add_doctype(elem.index),
                    HtmlTag::sys_text => {
                        let s = self.dom.string_at(elem.index);
                        if rawtext_depth > 0 {
                            self.buf.push_str(s);
                        } else {
                            self.buf.push_str(&entities::encode_text(s));
                        }
                    }
                    HtmlTag::sys_comment => self.buf.add_comment(self.dom.string_at(elem.index)),
                    _ => {
                        if matches!(tag, HtmlTag::script | HtmlTag::style) {
                            rawtext_depth += 1;
                        }
                        self.add_start_tag(elem.index, tag);
                    }
                },
                TagStage::Close => {
                    if matches!(tag, HtmlTag::script | HtmlTag::style) {
                        rawtext_depth = rawtext_depth.saturating_sub(1);
                    }
                    self.add_close_tag(elem.index);
                }
            }
        }
        self.buf.inner()
    }

    fn add_start_tag(&mut self, index: NodeIndex, tag: HtmlTag) {
        self.buf.push('<');
        // `tag_name` resolves extended (custom/unknown) tags to their real name; `tag` is the
        // normalized `HtmlTag` (`extended` for those) kept only for the `auto_close` check.
        self.buf.push_str(self.dom.tag_name(index));
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
        self.buf.push_str(self.dom.tag_name(index));
        self.buf.push('>');
    }
}
